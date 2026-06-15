use crate::graph::Pipe;
use crate::solver::gas_properties::GasComposition;

use super::boundary::{SinkBoundary, SourceBoundary};
use super::mesh::PipeMesh;
use super::state::TransientPipeState;
use super::system::{build_tridiagonal_step, solve_tridiagonal, update_flows};

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

/// Contexte d'une conduite active dans le réseau PDE MVP.
pub struct ActivePipeContext {
    pub pipe: Pipe,
    pub mesh: PipeMesh,
    pub state: TransientPipeState,
    pub source: SourceBoundary,
    pub sink: SinkBoundary,
}

/// Avance toutes les conduites actives d'un pas (couplage explicite aux jonctions pour chaîne).
pub fn advance_one_step(pipes: &mut [ActivePipeContext], dt_s: f64, composition: &GasComposition) {
    for ctx in pipes.iter_mut() {
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
