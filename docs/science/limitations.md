# Model limitations — GazFlow

This document describes the known limits of the solver in its current state. It complements `docs/science/equations.md` (model) and `docs/science/validation.md` (tests).

## 1. Physical limits (MVP)

- Steady state only (no temporal transient).
- Isothermal assumption (uniform temperature by default).
- Simplified gas properties depending on the active modelling level.
- Gravity/altitude effects not included in the main flow.
- Compressors modelled in MVP version (simplified representation).

## 2. Numerical limits

- Convergence depends on initialisation and relaxation parameters.
- Very large networks may require continuation strategies and warm-start.
- Convergence criterion remains sensitive to input data scaling.
- Some configurations may end in explicit non-convergence (smoke mode).

## 3. Data and validation limits

- Strict validation against an external official reference may be incomplete depending on the dataset.
- Internal reference is useful for non-regression but does not replace independent validation.
- Result quality depends directly on scenario and topology file quality.

## 4. Impact on usage

- The solver is suited for technical and comparative pipeline studies.
- Results must not be interpreted as a guarantee for industrial operation without business calibration.
- Critical decisions (safety, contracts, real-time control) require additional verification.

## 5. Recommended evolutions

- Extend external reference validation (pressure/flow) on a representative set of cases.
- Strengthen thermo-physical model (Z, viscosity, mixtures) according to business needs.
- Add gravity effects and complementary losses if required by use cases.
- Consolidate numerical robustness strategies for very large networks.
