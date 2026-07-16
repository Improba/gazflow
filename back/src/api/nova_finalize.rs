//! Point d'entrée API pour la finalisation du verdict NoVa (Newton post-hoc, escalade IPOPT).

use crate::solver::{self, NovaDiagnostics, NovaVerdict, SolverResult};

/// Dérive le verdict NoVa à partir des diagnostics et du résultat solveur local.
pub(crate) fn finalize_nova_verdict(
    diagnostics: &NovaDiagnostics,
    converged: bool,
    tol_m3s: f64,
    result: &SolverResult,
) -> NovaVerdict {
    solver::nova_verdict(diagnostics, converged, tol_m3s, result)
}
