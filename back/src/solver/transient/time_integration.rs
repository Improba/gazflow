use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::{ConnectionKind, Pipe};
use crate::solver::gas_properties::GasComposition;

use super::boundary::{SinkBoundary, SourceBoundary};
use super::mesh::PipeMesh;
use super::state::TransientPipeState;
use super::system::{build_tridiagonal_step, segment_conductance, solve_tridiagonal, update_flows};

/// Avance d'un pas implicite Euler sur une conduite maillée.
pub fn advance_pipe_one_step(
    mesh: &PipeMesh,
    state: &mut TransientPipeState,
    pipe: &Pipe,
    dt_s: f64,
    source: &SourceBoundary,
    sink: &SinkBoundary,
    composition: &GasComposition,
) {
    let step = build_tridiagonal_step(mesh, state, pipe, dt_s, source, sink, composition);
    state.pressures = solve_tridiagonal(&step);
    update_flows(mesh, state, pipe, source, sink, composition);
}

/// Contexte d'une conduite active dans le réseau PDE.
pub struct ActivePipeContext {
    pub pipe: Pipe,
    pub mesh: PipeMesh,
    pub state: TransientPipeState,
    pub source: SourceBoundary,
    pub sink: SinkBoundary,
}

/// Organe algébrique (pas de linepack) couplé au PDE.
#[derive(Debug, Clone)]
pub enum AlgebraicEquipment {
    /// Détendeur : impose P_aval ≈ consigne si P_amont le permet.
    Regulator {
        from: String,
        to: String,
        setpoint_bar: f64,
    },
    /// Compresseur MVP : P_aval = r · P_amont.
    Compressor {
        from: String,
        to: String,
        ratio: f64,
    },
}

const TREE_PICARD_MAX: usize = 20;
const TREE_PICARD_TOL_BAR: f64 = 1e-3;
const NODAL_PICARD_MAX: usize = 80;
const NODAL_PICARD_TOL_M3S: f64 = 1e-3;
pub(crate) const DEFAULT_PICARD_RELAX: f64 = 0.35;

/// Statut de convergence Picard pour un pas réseau.
#[derive(Debug, Clone, Copy)]
pub struct PicardStatus {
    pub converged: bool,
    pub iterations: usize,
    pub residual: f64,
}

/// Avance le réseau PDE d'un pas.
///
/// - **Arbre** : balayage feuilles→racine. `sink_flow = demande − Σ Qin_enfants − Σ Q_organes`.
/// - **Cycles** : arbre couvrant + cordes Dirichlet, même bilan (avec `chord_into`).
/// - **Organes** : contrainte P (consigne / ratio) + **transmission de débit**
///   (la demande aval est reportée sur l'amont pour le bilan massique).
///
/// Chaque itération Picard **re-résout le même dt** depuis l'état initial du pas.
///
/// Invariant fold / `equipment_outflow_from` : `fold_demands_through_equipment` reporte la
/// demande pure vers l'amont des organes ; `equipment_outflow_from` couvre les débits des
/// conduites maillées aval (enfants pipes) pour le bilan massique en amont.
pub fn advance_network_one_step(
    pipes: &mut [ActivePipeContext],
    node_pressures: &mut HashMap<String, f64>,
    fixed_pressure_nodes: &HashSet<String>,
    demands: &HashMap<String, f64>,
    dt_s: f64,
    composition: &GasComposition,
    is_tree: bool,
    equipment: &[AlgebraicEquipment],
    picard_relax: f64,
) -> PicardStatus {
    let mut effective_fixed = fixed_pressure_nodes.clone();
    apply_equipment_constraints(equipment, node_pressures, &mut effective_fixed);

    // Report des demandes à travers les organes (conservation : Q transmis A→B).
    let (effective_demands, fold_incomplete) = fold_demands_through_equipment(demands, equipment);
    let mut equip_incomplete = false;

    let mut status = if is_tree {
        advance_tree_sweep(
            pipes,
            node_pressures,
            &effective_fixed,
            &effective_demands,
            dt_s,
            composition,
            equipment,
            &mut equip_incomplete,
        )
    } else {
        advance_nodal_dirichlet(
            pipes,
            node_pressures,
            &effective_fixed,
            &effective_demands,
            dt_s,
            composition,
            equipment,
            &mut equip_incomplete,
            picard_relax,
        )
    };

    apply_equipment_constraints(equipment, node_pressures, &mut effective_fixed);
    if fold_incomplete || equip_incomplete {
        status.converged = false;
        status.residual = status.residual.max(1.0);
    }
    status
}

/// Reporte les demandes des nœuds aval d'organes vers l'amont (fixpoint).
///
/// Un régulateur/compresseur A→B transmet le débit sans stockage :
/// `demande_effective[A] += demande_effective[B]` puis `demande_effective[B] = 0`
/// pour le bilan des conduites maillées. La pression en B reste imposée par l'organe.
///
/// Retourne `(demandes_repliées, incomplete)` ; `incomplete` vaut true si une sortie
/// d'organe conserve encore une demande résiduelle après le nombre max d'itérations.
pub fn fold_demands_through_equipment(
    demands: &HashMap<String, f64>,
    equipment: &[AlgebraicEquipment],
) -> (HashMap<String, f64>, bool) {
    let mut d = demands.clone();
    if equipment.is_empty() {
        return (d, false);
    }
    // Point fixe : plusieurs organes en série.
    for _ in 0..equipment.len().saturating_mul(2).max(1) {
        let mut moved = false;
        for eq in equipment {
            let (from, to) = match eq {
                AlgebraicEquipment::Regulator { from, to, .. }
                | AlgebraicEquipment::Compressor { from, to, .. } => (from, to),
            };
            let q = d.get(to).copied().unwrap_or(0.0);
            if q.abs() > 1e-15 {
                *d.entry(from.clone()).or_insert(0.0) += q;
                d.insert(to.clone(), 0.0);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    // Cycle d'organes : la demande peut rester sur un outlet si le point fixe n'a pas convergé.
    let incomplete = equipment.iter().any(|eq| {
        let to = match eq {
            AlgebraicEquipment::Regulator { to, .. } | AlgebraicEquipment::Compressor { to, .. } => to,
        };
        d.get(to).copied().unwrap_or(0.0).abs() > 1e-12
    });
    if incomplete {
        eprintln!(
            "warning: fold_demands_through_equipment: residual demand on equipment outlet (equipment cycle?)"
        );
    }
    (d, incomplete)
}

fn advance_tree_sweep(
    pipes: &mut [ActivePipeContext],
    node_pressures: &mut HashMap<String, f64>,
    fixed_pressure_nodes: &HashSet<String>,
    demands: &HashMap<String, f64>,
    dt_s: f64,
    composition: &GasComposition,
    equipment: &[AlgebraicEquipment],
    equip_incomplete: &mut bool,
) -> PicardStatus {
    let snapshots: Vec<(Vec<f64>, Vec<f64>)> = pipes
        .iter()
        .map(|ctx| (ctx.state.pressures.clone(), ctx.state.flows.clone()))
        .collect();

    let children = children_by_parent_node(pipes);
    let mut effective_fixed = fixed_pressure_nodes.clone();
    let mut iterations = 0_usize;
    let mut last_residual = f64::INFINITY;
    let mut converged = false;

    for _ in 0..TREE_PICARD_MAX {
        iterations += 1;
        for (ctx, (pressures, flows)) in pipes.iter_mut().zip(snapshots.iter()) {
            ctx.state.pressures = pressures.clone();
            ctx.state.flows = flows.clone();
        }

        apply_equipment_constraints(equipment, node_pressures, &mut effective_fixed);
        let order = tree_leaf_to_root_order(pipes, &effective_fixed);

        let mut max_dp = 0.0_f64;
        let mut solved_qin: HashMap<String, f64> = HashMap::new();

        for &pipe_idx in &order {
            let from = pipes[pipe_idx].pipe.from.clone();
            let to = pipes[pipe_idx].pipe.to.clone();
            let p_from = node_pressures
                .get(&from)
                .copied()
                .unwrap_or(pipes[pipe_idx].source.pressure_bar);
            pipes[pipe_idx].source = SourceBoundary::fixed_pressure(p_from);

            let demand_to = demands.get(&to).copied().unwrap_or(0.0);
            let child_qin_sum: f64 = children
                .get(&to)
                .into_iter()
                .flatten()
                .filter_map(|child_idx| solved_qin.get(&pipes[*child_idx].pipe.id))
                .sum();
            let equip_out = equipment_outflow_from(
                &to,
                equipment,
                pipes,
                &solved_qin,
                demands,
                &children,
                equip_incomplete,
            );
            // Bilan en `to` : Q_arrive + demande = Σ Qin_enfants + Q_organes_sortants
            // Q_arrive = −sink_flow ⇒ sink_flow = demande − Σ Qin_enfants − Q_organes
            let sink_flow = demand_to - child_qin_sum - equip_out;
            pipes[pipe_idx].sink = SinkBoundary::fixed_flow(sink_flow);

            {
                let ctx = &mut pipes[pipe_idx];
                advance_pipe_one_step(
                    &ctx.mesh,
                    &mut ctx.state,
                    &ctx.pipe,
                    dt_s,
                    &ctx.source,
                    &ctx.sink,
                    composition,
                );
            }

            let ctx = &pipes[pipe_idx];
            let q_in = ctx.state.flows.first().copied().unwrap_or(0.0);
            solved_qin.insert(ctx.pipe.id.clone(), q_in);

            if !effective_fixed.contains(&to)
                && let Some(&p_end) = ctx.state.pressures.last()
            {
                let prev = node_pressures.get(&to).copied().unwrap_or(p_end);
                max_dp = max_dp.max((p_end - prev).abs());
                node_pressures.insert(to, p_end);
            }
            if !effective_fixed.contains(&from)
                && let Some(&p0) = ctx.state.pressures.first()
            {
                let prev = node_pressures.get(&from).copied().unwrap_or(p0);
                let blended = 0.5 * prev + 0.5 * p0;
                max_dp = max_dp.max((blended - prev).abs());
                node_pressures.insert(from, blended);
            }
        }

        apply_equipment_constraints(equipment, node_pressures, &mut effective_fixed);

        let free = free_nodes(pipes, &effective_fixed);
        let max_imb = free
            .iter()
            .map(|n| nodal_mass_imbalance(n, pipes, demands).abs())
            .fold(0.0_f64, f64::max);
        last_residual = max_imb;

        if max_dp < TREE_PICARD_TOL_BAR && max_imb < NODAL_PICARD_TOL_M3S {
            converged = true;
            break;
        }
    }

    PicardStatus {
        converged,
        iterations,
        residual: last_residual,
    }
}

/// Picard / balayage pour réseaux cycliques : arbre couvrant + cordes Dirichlet.
fn advance_nodal_dirichlet(
    pipes: &mut [ActivePipeContext],
    node_pressures: &mut HashMap<String, f64>,
    fixed_pressure_nodes: &HashSet<String>,
    demands: &HashMap<String, f64>,
    dt_s: f64,
    composition: &GasComposition,
    equipment: &[AlgebraicEquipment],
    equip_incomplete: &mut bool,
    picard_relax: f64,
) -> PicardStatus {
    let snapshots: Vec<(Vec<f64>, Vec<f64>)> = pipes
        .iter()
        .map(|ctx| (ctx.state.pressures.clone(), ctx.state.flows.clone()))
        .collect();

    let (tree_idxs, chord_idxs) = spanning_tree_partition(pipes, fixed_pressure_nodes);
    let mut effective_fixed = fixed_pressure_nodes.clone();
    let mut iterations = 0_usize;
    let mut last_residual = f64::INFINITY;
    let mut converged = false;

    for _ in 0..NODAL_PICARD_MAX {
        iterations += 1;
        for (ctx, (pressures, flows)) in pipes.iter_mut().zip(snapshots.iter()) {
            ctx.state.pressures = pressures.clone();
            ctx.state.flows = flows.clone();
        }

        apply_equipment_constraints(equipment, node_pressures, &mut effective_fixed);

        // 1) Cordes : Dirichlet–Dirichlet (fermeture de boucle).
        for &idx in &chord_idxs {
            let p_from = node_pressures
                .get(&pipes[idx].pipe.from)
                .copied()
                .unwrap_or(50.0);
            let p_to = node_pressures
                .get(&pipes[idx].pipe.to)
                .copied()
                .unwrap_or(p_from);
            pipes[idx].source = SourceBoundary::fixed_pressure(p_from);
            pipes[idx].sink = SinkBoundary::fixed_pressure(p_to);
            {
                let ctx = &mut pipes[idx];
                advance_pipe_one_step(
                    &ctx.mesh,
                    &mut ctx.state,
                    &ctx.pipe,
                    dt_s,
                    &ctx.source,
                    &ctx.sink,
                    composition,
                );
            }
        }

        // 2) Arbre : balayage feuilles→racine avec contribution des cordes.
        let order = tree_leaf_to_root_order_idxs(pipes, &tree_idxs, &effective_fixed);
        let children = children_by_parent_among(pipes, &tree_idxs);
        let mut solved_qin: HashMap<String, f64> = HashMap::new();

        for &pipe_idx in &order {
            let from = pipes[pipe_idx].pipe.from.clone();
            let to = pipes[pipe_idx].pipe.to.clone();
            let p_from = node_pressures.get(&from).copied().unwrap_or(50.0);
            pipes[pipe_idx].source = SourceBoundary::fixed_pressure(p_from);

            let demand_to = demands.get(&to).copied().unwrap_or(0.0);
            let child_qin_sum: f64 = children
                .get(&to)
                .into_iter()
                .flatten()
                .filter_map(|ci| solved_qin.get(&pipes[*ci].pipe.id))
                .sum();
            let chord_into = chord_net_flow_into(&to, pipes, &chord_idxs);
            let equip_out = equipment_outflow_from(
                &to,
                equipment,
                pipes,
                &solved_qin,
                demands,
                &children,
                equip_incomplete,
            );
            // Bilan : Q_arrive_tree + chord_into + demande = Σ child_qin + Q_organes
            // Q_arrive_tree = −sink_flow
            let sink_flow = if effective_fixed.contains(&to)
                && demand_to.abs() <= 1e-12
                && children.get(&to).is_none_or(|c| c.is_empty())
                && equip_out.abs() <= 1e-12
            {
                // Ancre aval pure (pas de prélèvement ni d'aval) : Dirichlet.
                pipes[pipe_idx].sink =
                    SinkBoundary::fixed_pressure(node_pressures.get(&to).copied().unwrap_or(p_from));
                {
                    let ctx = &mut pipes[pipe_idx];
                    advance_pipe_one_step(
                        &ctx.mesh,
                        &mut ctx.state,
                        &ctx.pipe,
                        dt_s,
                        &ctx.source,
                        &ctx.sink,
                        composition,
                    );
                }
                let q_in = pipes[pipe_idx].state.flows.first().copied().unwrap_or(0.0);
                solved_qin.insert(pipes[pipe_idx].pipe.id.clone(), q_in);
                continue;
            } else {
                demand_to + chord_into - child_qin_sum - equip_out
            };
            pipes[pipe_idx].sink = SinkBoundary::fixed_flow(sink_flow);

            {
                let ctx = &mut pipes[pipe_idx];
                advance_pipe_one_step(
                    &ctx.mesh,
                    &mut ctx.state,
                    &ctx.pipe,
                    dt_s,
                    &ctx.source,
                    &ctx.sink,
                    composition,
                );
            }

            let q_in = pipes[pipe_idx].state.flows.first().copied().unwrap_or(0.0);
            solved_qin.insert(pipes[pipe_idx].pipe.id.clone(), q_in);

            if !effective_fixed.contains(&to)
                && let Some(&p_end) = pipes[pipe_idx].state.pressures.last()
            {
                node_pressures.insert(to.clone(), p_end);
            }
            if !effective_fixed.contains(&from)
                && let Some(&p0) = pipes[pipe_idx].state.pressures.first()
            {
                let prev = node_pressures.get(&from).copied().unwrap_or(p0);
                node_pressures.insert(from, 0.5 * prev + 0.5 * p0);
            }
        }

        apply_equipment_constraints(equipment, node_pressures, &mut effective_fixed);

        let free = free_nodes(pipes, &effective_fixed);
        let max_imb = free
            .iter()
            .map(|n| nodal_mass_imbalance(n, pipes, demands).abs())
            .fold(0.0_f64, f64::max);
        last_residual = max_imb;
        if max_imb < NODAL_PICARD_TOL_M3S {
            converged = true;
            break;
        }

        // Correction nodale légère sur le résidu restant (cordes).
        for node in &free {
            let imbalance = nodal_mass_imbalance(node, pipes, demands);
            let g_eff =
                nodal_effective_conductance(node, pipes, node_pressures, composition).max(1e-3);
            let dp = (picard_relax * imbalance / g_eff).clamp(-2.0, 2.0);
            let p = node_pressures.entry(node.clone()).or_insert(50.0);
            *p = (*p + dp).clamp(1.0, 200.0);
        }
    }

    PicardStatus {
        converged,
        iterations,
        residual: last_residual,
    }
}

fn spanning_tree_partition(
    pipes: &[ActivePipeContext],
    fixed_pressure_nodes: &HashSet<String>,
) -> (Vec<usize>, Vec<usize>) {
    // BFS undirected from fixed-pressure seeds; first edge to a node enters the tree.
    let mut adj: HashMap<&str, Vec<(usize, &str)>> = HashMap::new();
    for (idx, ctx) in pipes.iter().enumerate() {
        adj.entry(ctx.pipe.from.as_str())
            .or_default()
            .push((idx, ctx.pipe.to.as_str()));
        adj.entry(ctx.pipe.to.as_str())
            .or_default()
            .push((idx, ctx.pipe.from.as_str()));
    }
    for neighbors in adj.values_mut() {
        neighbors.sort_by(|a, b| a.1.cmp(b.1).then(a.0.cmp(&b.0)));
    }
    let mut parent_edge: HashMap<&str, usize> = HashMap::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut q = VecDeque::new();
    let mut seeds: Vec<&str> = fixed_pressure_nodes.iter().map(|s| s.as_str()).collect();
    seeds.sort_unstable();
    for n in seeds {
        if seen.insert(n) {
            q.push_back(n);
        }
    }
    if q.is_empty()
        && let Some(ctx) = pipes.first()
    {
        seen.insert(ctx.pipe.from.as_str());
        q.push_back(ctx.pipe.from.as_str());
    }
    while let Some(n) = q.pop_front() {
        if let Some(neis) = adj.get(n) {
            for &(eidx, m) in neis {
                if seen.insert(m) {
                    parent_edge.insert(m, eidx);
                    q.push_back(m);
                }
            }
        }
    }
    let tree_set: HashSet<usize> = parent_edge.values().copied().collect();
    let mut tree = Vec::new();
    let mut chords = Vec::new();
    for i in 0..pipes.len() {
        if tree_set.contains(&i) {
            tree.push(i);
        } else {
            chords.push(i);
        }
    }
    // Si BFS n'a rien capturé (réseau bizarre), tout en arbre.
    if tree.is_empty() {
        tree = (0..pipes.len()).collect();
        chords.clear();
    }
    (tree, chords)
}

fn tree_leaf_to_root_order_idxs(
    pipes: &[ActivePipeContext],
    tree_idxs: &[usize],
    fixed_pressure_nodes: &HashSet<String>,
) -> Vec<usize> {
    let mut adj: HashMap<&str, Vec<(usize, &str)>> = HashMap::new();
    for &idx in tree_idxs {
        let ctx = &pipes[idx];
        adj.entry(ctx.pipe.from.as_str())
            .or_default()
            .push((idx, ctx.pipe.to.as_str()));
        adj.entry(ctx.pipe.to.as_str())
            .or_default()
            .push((idx, ctx.pipe.from.as_str()));
    }
    let mut dist: HashMap<&str, usize> = HashMap::new();
    let mut q = VecDeque::new();
    for n in fixed_pressure_nodes {
        dist.insert(n.as_str(), 0);
        q.push_back(n.as_str());
    }
    while let Some(n) = q.pop_front() {
        let d = dist[&n];
        if let Some(neis) = adj.get(n) {
            for &(_, m) in neis {
                if let std::collections::hash_map::Entry::Vacant(e) = dist.entry(m) {
                    e.insert(d + 1);
                    q.push_back(m);
                }
            }
        }
    }
    let mut order = tree_idxs.to_vec();
    order.sort_by(|&a, &b| {
        let da = dist
            .get(pipes[a].pipe.to.as_str())
            .copied()
            .unwrap_or(0)
            .max(dist.get(pipes[a].pipe.from.as_str()).copied().unwrap_or(0));
        let db = dist
            .get(pipes[b].pipe.to.as_str())
            .copied()
            .unwrap_or(0)
            .max(dist.get(pipes[b].pipe.from.as_str()).copied().unwrap_or(0));
        db.cmp(&da)
            .then_with(|| pipes[a].pipe.id.cmp(&pipes[b].pipe.id))
    });
    order
}

fn children_by_parent_among(
    pipes: &[ActivePipeContext],
    tree_idxs: &[usize],
) -> HashMap<String, Vec<usize>> {
    let mut map: HashMap<String, Vec<usize>> = HashMap::new();
    for &idx in tree_idxs {
        map.entry(pipes[idx].pipe.from.clone())
            .or_default()
            .push(idx);
    }
    map
}

fn chord_net_flow_into(
    node: &str,
    pipes: &[ActivePipeContext],
    chord_idxs: &[usize],
) -> f64 {
    let mut net = 0.0;
    for &idx in chord_idxs {
        let ctx = &pipes[idx];
        if ctx.pipe.from == node {
            net -= ctx.state.flows.first().copied().unwrap_or(0.0);
        }
        if ctx.pipe.to == node {
            net += ctx.state.flows.last().copied().unwrap_or(0.0);
        }
    }
    net
}

/// Débit [Nm³/s] que les organes partant de `node` doivent transmettre vers l'aval.
///
/// Pour un organe A→B : `Q_e + demande_B = Σ Qin_enfants(B) + Q_organes(B)`
/// donc `Q_e = Σ Qin_enfants(B) + Q_organes(B) − demande_B`.
fn equipment_outflow_from(
    node: &str,
    equipment: &[AlgebraicEquipment],
    pipes: &[ActivePipeContext],
    solved_qin: &HashMap<String, f64>,
    demands: &HashMap<String, f64>,
    children: &HashMap<String, Vec<usize>>,
    incomplete: &mut bool,
) -> f64 {
    equipment_outflow_from_limited(
        node,
        equipment,
        pipes,
        solved_qin,
        demands,
        children,
        0,
        incomplete,
    )
}

fn equipment_outflow_from_limited(
    node: &str,
    equipment: &[AlgebraicEquipment],
    pipes: &[ActivePipeContext],
    solved_qin: &HashMap<String, f64>,
    demands: &HashMap<String, f64>,
    children: &HashMap<String, Vec<usize>>,
    depth: usize,
    incomplete: &mut bool,
) -> f64 {
    if depth > equipment.len().saturating_add(1) {
        eprintln!(
            "warning: equipment_outflow_from: depth limit exceeded at node {node} (partial total returned)"
        );
        *incomplete = true;
        // Branche aval non résolue ; le total partiel accumulé par l'appelant est conservé.
        return 0.0;
    }
    let mut total = 0.0;
    for eq in equipment {
        let (from, to) = match eq {
            AlgebraicEquipment::Regulator { from, to, .. }
            | AlgebraicEquipment::Compressor { from, to, .. } => (from.as_str(), to.as_str()),
        };
        if from != node {
            continue;
        }
        let child_qin: f64 = children
            .get(to)
            .into_iter()
            .flatten()
            .filter_map(|i| solved_qin.get(&pipes[*i].pipe.id))
            .sum();
        let further = equipment_outflow_from_limited(
            to,
            equipment,
            pipes,
            solved_qin,
            demands,
            children,
            depth + 1,
            incomplete,
        );
        let demand_to = demands.get(to).copied().unwrap_or(0.0);
        total += child_qin + further - demand_to;
    }
    total
}

fn apply_equipment_constraints(
    equipment: &[AlgebraicEquipment],
    node_pressures: &mut HashMap<String, f64>,
    fixed: &mut HashSet<String>,
) {
    for eq in equipment {
        match eq {
            AlgebraicEquipment::Regulator {
                from,
                to,
                setpoint_bar,
            } => {
                let p_up = node_pressures.get(from).copied().unwrap_or(*setpoint_bar);
                // Régulation active si amont au-dessus de la consigne.
                let p_down = if p_up > *setpoint_bar + 0.1 {
                    *setpoint_bar
                } else {
                    // Bypass / perte de régulation : aval suit amont (MVP).
                    p_up
                };
                node_pressures.insert(to.clone(), p_down.clamp(1.0, 200.0));
                fixed.insert(to.clone());
            }
            AlgebraicEquipment::Compressor { from, to, ratio } => {
                let p_up = node_pressures.get(from).copied().unwrap_or(50.0);
                let r = ratio.max(1.0);
                let p_down = (p_up * r).clamp(1.0, 200.0);
                node_pressures.insert(to.clone(), p_down);
                fixed.insert(to.clone());
            }
        }
    }
}

fn free_nodes(pipes: &[ActivePipeContext], fixed: &HashSet<String>) -> Vec<String> {
    let mut nodes: HashSet<String> = HashSet::new();
    for ctx in pipes {
        nodes.insert(ctx.pipe.from.clone());
        nodes.insert(ctx.pipe.to.clone());
    }
    let mut free: Vec<String> = nodes.into_iter().filter(|n| !fixed.contains(n)).collect();
    free.sort();
    free
}

fn children_by_parent_node(pipes: &[ActivePipeContext]) -> HashMap<String, Vec<usize>> {
    let mut map: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, ctx) in pipes.iter().enumerate() {
        map.entry(ctx.pipe.from.clone()).or_default().push(idx);
    }
    map
}

fn tree_leaf_to_root_order(
    pipes: &[ActivePipeContext],
    fixed_pressure_nodes: &HashSet<String>,
) -> Vec<usize> {
    let mut adj: HashMap<&str, Vec<(usize, &str)>> = HashMap::new();
    for (idx, ctx) in pipes.iter().enumerate() {
        adj.entry(ctx.pipe.from.as_str())
            .or_default()
            .push((idx, ctx.pipe.to.as_str()));
        adj.entry(ctx.pipe.to.as_str())
            .or_default()
            .push((idx, ctx.pipe.from.as_str()));
    }

    for neighbors in adj.values_mut() {
        neighbors.sort_by(|a, b| a.1.cmp(b.1).then(a.0.cmp(&b.0)));
    }

    let mut dist: HashMap<&str, usize> = HashMap::new();
    let mut q: VecDeque<&str> = VecDeque::new();
    let mut seeds: Vec<&str> = fixed_pressure_nodes.iter().map(|s| s.as_str()).collect();
    seeds.sort_unstable();
    for n in seeds {
        dist.insert(n, 0);
        q.push_back(n);
    }
    if q.is_empty() {
        return (0..pipes.len()).collect();
    }
    while let Some(n) = q.pop_front() {
        let d = dist[&n];
        if let Some(neis) = adj.get(n) {
            for &(_, m) in neis {
                if let std::collections::hash_map::Entry::Vacant(e) = dist.entry(m) {
                    e.insert(d + 1);
                    q.push_back(m);
                }
            }
        }
    }

    let mut order: Vec<usize> = (0..pipes.len()).collect();
    order.sort_by(|&a, &b| {
        let da = dist
            .get(pipes[a].pipe.to.as_str())
            .copied()
            .unwrap_or(0)
            .max(dist.get(pipes[a].pipe.from.as_str()).copied().unwrap_or(0));
        let db = dist
            .get(pipes[b].pipe.to.as_str())
            .copied()
            .unwrap_or(0)
            .max(dist.get(pipes[b].pipe.from.as_str()).copied().unwrap_or(0));
        db.cmp(&da)
            .then_with(|| pipes[a].pipe.id.cmp(&pipes[b].pipe.id))
    });
    order
}

/// Déséquilibre massique nodal [Nm³/s] : ΣQ_arrivant − ΣQ_partant + demande.
pub fn nodal_mass_imbalance(
    node: &str,
    pipes: &[ActivePipeContext],
    demands: &HashMap<String, f64>,
) -> f64 {
    let mut net = demands.get(node).copied().unwrap_or(0.0);
    for ctx in pipes {
        if ctx.pipe.from == node {
            net -= ctx.state.flows.first().copied().unwrap_or(0.0);
        }
        if ctx.pipe.to == node {
            net += ctx.state.flows.last().copied().unwrap_or(0.0);
        }
    }
    net
}

fn nodal_effective_conductance(
    node: &str,
    pipes: &[ActivePipeContext],
    node_pressures: &HashMap<String, f64>,
    composition: &GasComposition,
) -> f64 {
    let mut g_sum = 0.0;
    let p_node = node_pressures.get(node).copied().unwrap_or(50.0);
    for ctx in pipes {
        if ctx.pipe.from == node {
            let p_cell = ctx.state.pressures.first().copied().unwrap_or(p_node);
            let q = ctx.state.flows.first().copied().unwrap_or(0.0);
            g_sum += segment_conductance(
                &ctx.pipe,
                0.5 * (p_node + p_cell),
                ctx.mesh.dx * 1e-3,
                composition,
                q,
            );
        }
        if ctx.pipe.to == node {
            let p_cell = ctx.state.pressures.last().copied().unwrap_or(p_node);
            let q = ctx.state.flows.last().copied().unwrap_or(0.0);
            g_sum += segment_conductance(
                &ctx.pipe,
                0.5 * (p_cell + p_node),
                ctx.mesh.dx * 1e-3,
                composition,
                q,
            );
        }
    }
    g_sum
}

/// Estime un dt adaptatif (précision, pas stabilité : schéma déjà implicite).
///
/// Critère principal : `dt ≤ α · min_i (C_i / G_i)` (constante de temps stockage /
/// conductance), α = 0.5. Un plafond secondaire `β · dx / c` (β = 50, c ≈ 350 m/s)
/// évite des pas absurdes sur maillage très fin ; **ce n'est pas un CFL de stabilité**.
///
/// Plancher : `max(1 s, 5 % de dt_max)` pour borner le nombre de pas (le schéma
/// implicite n'a pas besoin de sous-secondes en exploitation isotherme).
pub fn suggest_adaptive_dt_s(
    pipes: &[ActivePipeContext],
    composition: &GasComposition,
    dt_max_s: f64,
    remaining_s: f64,
) -> f64 {
    const ALPHA: f64 = 0.5;
    const C_WAVE_M_S: f64 = 350.0;
    const WAVE_HINT_FACTOR: f64 = 50.0;

    let dt_floor = (dt_max_s * 0.05).max(1.0).min(dt_max_s.max(1.0));
    let mut dt = dt_max_s.min(remaining_s.max(dt_floor));

    for ctx in pipes {
        let n = ctx.mesh.n_cells;
        if n == 0 {
            continue;
        }
        let dx = ctx.mesh.dx;
        dt = dt.min((dx / C_WAVE_M_S) * WAVE_HINT_FACTOR);

        for i in 0..n {
            let p = ctx.state.pressures.get(i).copied().unwrap_or(50.0);
            let c = super::system::storage_capacitance_nm3_per_bar(
                composition,
                p,
                crate::solver::gas_properties::DEFAULT_GAS_TEMPERATURE_K,
                ctx.mesh.area_m2,
                dx,
            );
            let q = ctx.state.flows.get(i).copied().unwrap_or(0.0);
            let g = segment_conductance(
                &ctx.pipe,
                p.max(1.0),
                dx * 1e-3,
                composition,
                q,
            );
            if g > 1e-12 && c > 1e-12 {
                dt = dt.min(ALPHA * c / g);
            }
        }
    }
    dt.clamp(dt_floor, dt_max_s.max(dt_floor))
        .min(remaining_s.max(dt_floor))
}

/// True si l'arc est maillable PDE (linepack).
pub fn is_pde_meshable(kind: ConnectionKind) -> bool {
    matches!(
        kind,
        ConnectionKind::Pipe
            | ConnectionKind::ShortPipe
            | ConnectionKind::Resistor
            | ConnectionKind::Valve
    )
}
