//! Phase VIII (option B) — Backend IPOPT externe sur le modèle in-repo exact.
//!
//! Résout le NLP de faisabilité NoVa (variables π = P² bornées par les bornes
//! natives `.net`, contraintes = bilan massique aux nœuds libres) avec IPOPT
//! via l'API C stable (`IpStdCInterface.h`, Ipopt 3.11.9), Hessian en
//! limited-memory (L-BFGS) — aucun Hessian à fournir. Les résidus et le
//! Jacobien sont évalués par l'oracle `pressure_nlp_eval` qui réutilise
//! `evaluate_state` : le modèle résout est donc **strictement** le modèle in-repo
//! (ρ dynamique Papay, gravité, aliasing ShortPipe, couplage compresseur cuit),
//! sans la pénalité (bornes dures sur x au lieu de pénalités molles).
//!
//! Limite méthodologique (ZIB) : IPOPT est un solveur local ; il ne peut pas
//! prouver l'infeasibilité globale. Un échec est donc `NotSolvedLocal`, jamais
//! `Infeasible` (preuve). Un succès (résidu ~0 + bornes respectées) est
//! `Feasible` (preuve constructive).

#![cfg(feature = "nlp-ipopt")]

use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_double, c_int, c_void};

use anyhow::{Result, bail};

use crate::graph::GasNetwork;
use crate::solver::gas_properties::GasComposition;

use super::newton::{pressure_nlp_eval, pressure_nlp_structure, PressureNlpStructure};

// ---- Types C Ipopt (IpStdCInterface.h, Ipopt 3.11.9) ----
type Index = c_int;
type Number = c_double;
type Bool = c_int;
type IpoptProblem = *mut c_void;
type UserDataPtr = *mut c_void;

// Codes de retour (IpReturnCodes_inc.h).
const SOLVE_SUCCEEDED: Index = 0;
const SOLVED_TO_ACCEPTABLE_LEVEL: Index = 1;
const INFEASIBLE_PROBLEM_DETECTED: Index = 2;
const FEASIBLE_POINT_FOUND: Index = 6;

// Fonction pointer types des callbacks.
type EvalF = Option<extern "C" fn(Index, *mut Number, Bool, *mut Number, UserDataPtr) -> Bool>;
type EvalGradF = Option<extern "C" fn(Index, *mut Number, Bool, *mut Number, UserDataPtr) -> Bool>;
type EvalG = Option<extern "C" fn(Index, *mut Number, Bool, Index, *mut Number, UserDataPtr) -> Bool>;
type EvalJacG = Option<
    extern "C" fn(
        Index,
        *mut Number,
        Bool,
        Index,
        Index,
        *mut Index,
        *mut Index,
        *mut Number,
        UserDataPtr,
    ) -> Bool,
>;
type EvalH = Option<
    extern "C" fn(
        Index,
        *mut Number,
        Bool,
        Number,
        Index,
        *mut Number,
        Bool,
        Index,
        *mut Index,
        *mut Index,
        *mut Number,
        UserDataPtr,
    ) -> Bool,
>;
type IntermediateCB = Option<
    extern "C" fn(
        Index,
        Index,
        Number,
        Number,
        Number,
        Number,
        Number,
        Number,
        Number,
        Number,
        Index,
        UserDataPtr,
    ) -> Bool,
>;

unsafe extern "C" {
    fn CreateIpoptProblem(
        n: Index,
        x_L: *mut Number,
        x_U: *mut Number,
        m: Index,
        g_L: *mut Number,
        g_U: *mut Number,
        nele_jac: Index,
        nele_hess: Index,
        index_style: Index,
        eval_f: EvalF,
        eval_g: EvalG,
        eval_grad_f: EvalGradF,
        eval_jac_g: EvalJacG,
        eval_h: EvalH,
    ) -> IpoptProblem;
    fn FreeIpoptProblem(ipopt_problem: IpoptProblem);
    fn AddIpoptStrOption(p: IpoptProblem, k: *mut c_char, v: *mut c_char) -> Bool;
    fn AddIpoptNumOption(p: IpoptProblem, k: *mut c_char, v: Number) -> Bool;
    fn AddIpoptIntOption(p: IpoptProblem, k: *mut c_char, v: Index) -> Bool;
    fn OpenIpoptOutputFile(p: IpoptProblem, file_name: *mut c_char, print_level: Index) -> Bool;
    fn SetIntermediateCallback(p: IpoptProblem, cb: IntermediateCB) -> Bool;
    fn IpoptSolve(
        p: IpoptProblem,
        x: *mut Number,
        g: *mut Number,
        obj_val: *mut Number,
        mult_g: *mut Number,
        mult_x_L: *mut Number,
        mult_x_U: *mut Number,
        user_data: UserDataPtr,
    ) -> Index;
}

/// Verdict de faisabilité NoVa retourné par le backend IPOPT.
#[derive(Debug, Clone)]
pub enum NovaIpoptVerdict {
    /// IPOPT a convergé (résidu ~0) ET toutes les bornes natives sont respectées.
    Feasible {
        pressures_bar: HashMap<String, f64>,
        residual_inf: f64,
        iterations: i32,
        status: i32,
    },
    /// IPOPT a convergé mais au moins une borne native est violée.
    BoundViolation {
        pressures_bar: HashMap<String, f64>,
        residual_inf: f64,
        iterations: i32,
        status: i32,
        max_violation_bar: f64,
    },
    /// IPOPT n'a pas convergé (local). Un solveur local ne peut pas prouver
    /// l'infeasibilité globale : on ne déclare jamais `Infeasible`.
    NotSolvedLocal {
        status: i32,
        message: String,
    },
    /// Erreur technique (problème de définition, mémoire, exception interne).
    Error {
        status: i32,
        message: String,
    },
}

/// Options du solveur IPOPT.
#[derive(Debug, Clone)]
pub struct NovaIpoptOptions {
    pub max_iter: i32,
    pub tol: f64,
    pub constr_viol_tol: f64,
    pub print_level: i32,
    pub output_file: Option<String>,
    pub initial_pressures_bar: Option<HashMap<String, f64>>,
}

impl Default for NovaIpoptOptions {
    fn default() -> Self {
        Self {
            max_iter: 4000,
            tol: 1e-6,
            constr_viol_tol: 1e-3,
            print_level: 5,
            output_file: None,
            initial_pressures_bar: None,
        }
    }
}

/// Contexte propriétaire passé aux callbacks via `UserDataPtr`.
struct NlpContext {
    network: GasNetwork,
    demands: HashMap<String, f64>,
    gas: GasComposition,
    structure: PressureNlpStructure,
}

unsafe fn ctx<'a>(user_data: UserDataPtr) -> &'a NlpContext {
    unsafe { &*(user_data as *const NlpContext) }
}

// ---- Callbacks (fonction pointers `extern "C"`) ----

extern "C" fn eval_f(
    _n: Index,
    _x: *mut Number,
    _new_x: Bool,
    obj_value: *mut Number,
    _user_data: UserDataPtr,
) -> Bool {
    unsafe { *obj_value = 0.0 };
    1
}

extern "C" fn eval_grad_f(
    n: Index,
    _x: *mut Number,
    _new_x: Bool,
    grad_f: *mut Number,
    _user_data: UserDataPtr,
) -> Bool {
    unsafe {
        std::ptr::write_bytes(grad_f, 0u8, n as usize);
    }
    1
}

extern "C" fn eval_g(
    n: Index,
    x: *mut Number,
    _new_x: Bool,
    m: Index,
    g: *mut Number,
    user_data: UserDataPtr,
) -> Bool {
    let c = unsafe { ctx(user_data) };
    let x_slice = unsafe { std::slice::from_raw_parts(x, n as usize) };
    let eval = match pressure_nlp_eval(&c.network, &c.demands, c.gas, x_slice) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    if eval.g.len() != m as usize {
        return 0;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(eval.g.as_ptr(), g, m as usize);
    }
    1
}

extern "C" fn eval_jac_g(
    n: Index,
    x: *mut Number,
    _new_x: Bool,
    m: Index,
    nele_jac: Index,
    iRow: *mut Index,
    jCol: *mut Index,
    values: *mut Number,
    user_data: UserDataPtr,
) -> Bool {
    let c = unsafe { ctx(user_data) };
    // Mode structure (values == NULL) : remplir iRow/jCol depuis la sparsité.
    if values.is_null() {
        let nnz = c.structure.jac_row.len();
        if nnz != nele_jac as usize {
            return 0;
        }
        unsafe {
            for (k, &r) in c.structure.jac_row.iter().enumerate() {
                *iRow.add(k) = r as Index;
                *jCol.add(k) = c.structure.jac_col[k] as Index;
            }
        }
        return 1;
    }
    // Mode valeurs : évaluer le Jacobien à x.
    let x_slice = unsafe { std::slice::from_raw_parts(x, n as usize) };
    let eval = match pressure_nlp_eval(&c.network, &c.demands, c.gas, x_slice) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    if eval.jac_val.len() != nele_jac as usize {
        return 0;
    }
    let _ = m;
    unsafe {
        std::ptr::copy_nonoverlapping(eval.jac_val.as_ptr(), values, nele_jac as usize);
    }
    1
}

// Hessian : no-op (limited-memory). Non appelé avec hessian_approximation=limited-memory.
extern "C" fn eval_h(
    _n: Index,
    _x: *mut Number,
    _new_x: Bool,
    _obj_factor: Number,
    _m: Index,
    _lambda: *mut Number,
    _new_lambda: Bool,
    _nele_hess: Index,
    _iRow: *mut Index,
    _jCol: *mut Index,
    _values: *mut Number,
    _user_data: UserDataPtr,
) -> Bool {
    1
}

extern "C" fn intermediate_cb(
    _alg_mod: Index,
    iter_count: Index,
    _obj_value: Number,
    inf_pr: Number,
    _inf_du: Number,
    _mu: Number,
    _d_norm: Number,
    _regularization_size: Number,
    _alpha_du: Number,
    _alpha_pr: Number,
    _ls_trials: Index,
    _user_data: UserDataPtr,
) -> Bool {
    if iter_count % 25 == 0 {
        eprintln!(
            "[ipopt] iter={iter_count:4}  inf_pr={inf_pr:.3e}"
        );
    }
    1
}

/// Résout le NLP NoVa (modèle in-repo) avec IPOPT.
pub fn solve_nova_with_ipopt(
    network: &GasNetwork,
    demands: &HashMap<String, f64>,
    gas: GasComposition,
    options: &NovaIpoptOptions,
) -> Result<NovaIpoptVerdict> {
    let structure = pressure_nlp_structure(network, demands, gas)?;
    let n = structure.free_node_ids.len();
    let m = structure.num_constraints;
    let nnz = structure.jac_row.len();
    if n == 0 {
        bail!("NLP NoVa/IPOPT: aucune variable libre");
    }
    if m != n {
        // Cas anormal (composante flottante ancrée en plus du slack) — on l'accepte,
        // IPOPT gère m ≠ n, mais on l'indique.
        tracing::debug!("NLP NoVa/IPOPT: n={n} m={m} (non carré, ancrage flottant)");
    }

    // Bornes variables (π²) et contraintes (égalités g = 0).
    let mut x_lo = structure.var_lo.clone();
    let mut x_hi = structure.var_hi.clone();
    // Point de départ.
    let mut x = vec![70.0_f64.powi(2); n];
    if let Some(init) = &options.initial_pressures_bar {
        for (i, id) in structure.free_node_ids.iter().enumerate() {
            if let Some(&p) = init.get(id) {
                if p.is_finite() && p > 0.0 {
                    x[i] = p * p;
                }
            }
        }
    }
    // Bornes contraintes : égalités (g_L = g_U = 0).
    let g_lo = vec![0.0_f64; m];
    let g_hi = vec![0.0_f64; m];

    let context = Box::new(NlpContext {
        network: network.clone(),
        demands: demands.clone(),
        gas,
        structure,
    });
    let user_data = Box::into_raw(context) as UserDataPtr;

    let problem = unsafe {
        CreateIpoptProblem(
            n as Index,
            x_lo.as_mut_ptr(),
            x_hi.as_mut_ptr(),
            m as Index,
            g_lo.as_ptr() as *mut Number,
            g_hi.as_ptr() as *mut Number,
            nnz as Index,
            0, // nele_hess (limited-memory)
            0, // index_style = C (0-based)
            Some(eval_f),
            Some(eval_g),
            Some(eval_grad_f),
            Some(eval_jac_g),
            Some(eval_h),
        )
    };
    if problem.is_null() {
        unsafe { drop(Box::from_raw(user_data as *mut NlpContext)) };
        bail!("CreateIpoptProblem a retourné NULL");
    }

    // Options. On garde les CString en vie jusqu'à la fin du bloc.
    let mut ok = true;
    let mut cstrings: Vec<CString> = Vec::new();
    macro_rules! str_opt {
        ($k:expr, $v:expr) => {{
            let kk = CString::new($k).unwrap();
            let vv = CString::new($v).unwrap();
            let r = unsafe { AddIpoptStrOption(problem, kk.as_ptr() as *mut c_char, vv.as_ptr() as *mut c_char) } != 0;
            cstrings.push(kk);
            cstrings.push(vv);
            ok &= r;
            r
        }};
    }
    macro_rules! num_opt {
        ($k:expr, $v:expr) => {{
            let kk = CString::new($k).unwrap();
            let r = unsafe { AddIpoptNumOption(problem, kk.as_ptr() as *mut c_char, $v) } != 0;
            cstrings.push(kk);
            ok &= r;
            r
        }};
    }
    macro_rules! int_opt {
        ($k:expr, $v:expr) => {{
            let kk = CString::new($k).unwrap();
            let r = unsafe { AddIpoptIntOption(problem, kk.as_ptr() as *mut c_char, $v) } != 0;
            cstrings.push(kk);
            ok &= r;
            r
        }};
    }
    str_opt!("hessian_approximation", "limited-memory");
    str_opt!("mu_strategy", "adaptive");
    int_opt!("max_iter", options.max_iter);
    num_opt!("tol", options.tol);
    num_opt!("constr_viol_tol", options.constr_viol_tol);
    int_opt!("print_level", options.print_level);
    if let Some(path) = &options.output_file {
        let cpath = CString::new(path.as_str()).unwrap();
        unsafe { OpenIpoptOutputFile(problem, cpath.as_ptr() as *mut c_char, options.print_level) };
        cstrings.push(cpath);
    }
    if !ok {
        eprintln!("[ipopt] attention : une ou plusieurs options n'ont pas pu être positionnées");
    }
    drop(cstrings);

    // Callback intermédiaire : log périodique (inf_pr) + arrêt utilisateur possible.
    unsafe { SetIntermediateCallback(problem, Some(intermediate_cb)) };

    let mut g_out = vec![0.0_f64; m];
    let mut obj_val = 0.0_f64;
    let status = unsafe {
        IpoptSolve(
            problem,
            x.as_mut_ptr(),
            g_out.as_mut_ptr(),
            &mut obj_val as *mut Number,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            user_data,
        )
    };

    unsafe { FreeIpoptProblem(problem) };
    let context = unsafe { Box::from_raw(user_data as *mut NlpContext) };
    let structure = &context.structure;

    // Récupère les pressions (free depuis x, fixed depuis le réseau).
    let mut pressures_bar: HashMap<String, f64> = HashMap::new();
    for node in network.nodes() {
        if let Some(p) = node.pressure_fixed_bar {
            pressures_bar.insert(node.id.clone(), p);
        }
    }
    for (i, id) in structure.free_node_ids.iter().enumerate() {
        let p_sq = x[i].max(0.0);
        pressures_bar.insert(id.clone(), p_sq.sqrt());
    }

    // Résidu final + violations de bornes natives.
    let final_eval = pressure_nlp_eval(network, demands, gas, &x).ok();
    let residual_inf = final_eval.map(|e| e.residual_inf).unwrap_or(f64::INFINITY);
    let mut max_violation_bar = 0.0_f64;
    for (i, _id) in structure.free_node_ids.iter().enumerate() {
        let p = x[i].max(0.0).sqrt();
        let lo = structure.var_lo[i].max(0.0).sqrt();
        let hi = structure.var_hi[i].min(1e20).sqrt();
        if p < lo {
            max_violation_bar = max_violation_bar.max(lo - p);
        }
        if hi.is_finite() && p > hi {
            max_violation_bar = max_violation_bar.max(p - hi);
        }
    }

    let success_status = matches!(
        status,
        SOLVE_SUCCEEDED | SOLVED_TO_ACCEPTABLE_LEVEL | FEASIBLE_POINT_FOUND
    );
    let infeasible_status = status == INFEASIBLE_PROBLEM_DETECTED;

    // Itérations : non exposé par l'API C 3.11 sans statistiques ; on met status.
    let iterations = -1;

    let verdict = if success_status && residual_inf <= options.constr_viol_tol.max(1e-3) && max_violation_bar < 1e-3 {
        NovaIpoptVerdict::Feasible {
            pressures_bar,
            residual_inf,
            iterations,
            status,
        }
    } else if success_status && max_violation_bar >= 1e-3 {
        NovaIpoptVerdict::BoundViolation {
            pressures_bar,
            residual_inf,
            iterations,
            status,
            max_violation_bar,
        }
    } else if infeasible_status {
        NovaIpoptVerdict::NotSolvedLocal {
            status,
            message: "IPOPT: Infeasible_Problem_Detected (local — ne prouve pas l'infeasibilité globale)".into(),
        }
    } else if status >= 0 {
        // Convergence logicielle mais résidu non nul : non résolu localement.
        NovaIpoptVerdict::NotSolvedLocal {
            status,
            message: format!(
                "IPOPT status={status} residual_inf={residual_inf:.3e} max_viol={max_violation_bar:.3e}"
            ),
        }
    } else {
        NovaIpoptVerdict::Error {
            status,
            message: format!("IPOPT erreur (code {status}) residual_inf={residual_inf:.3e}"),
        }
    };

    Ok(verdict)
}
