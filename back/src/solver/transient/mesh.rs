use crate::graph::Pipe;

/// Maillage 1D uniforme d'une conduite pour le transitoire PDE.
#[derive(Debug, Clone, Copy)]
pub struct PipeMesh {
    pub n_cells: usize,
    pub dx: f64,
    pub diameter_m: f64,
    pub length_m: f64,
    pub area_m2: f64,
}

/// Nombre minimal de cellules par conduite.
pub const MIN_CELLS: usize = 4;

/// Cible de cellules par kilomètre (au moins [`MIN_CELLS`]).
pub const CELLS_PER_KM: f64 = 2.0;

/// Nombre maximal de cellules par conduite (MVP).
pub const MAX_CELLS: usize = 64;

impl PipeMesh {
    /// Construit un maillage uniforme à partir d'une conduite.
    pub fn from_pipe(pipe: &Pipe, n_cells: Option<usize>) -> Self {
        let length_m = (pipe.length_km * 1e3).max(1.0);
        let diameter_m = (pipe.diameter_mm * 1e-3).max(1e-6);
        let n_cells = n_cells.unwrap_or_else(|| default_n_cells(pipe.length_km));
        let dx = length_m / n_cells as f64;
        let area_m2 = std::f64::consts::PI * diameter_m * diameter_m / 4.0;
        Self {
            n_cells,
            dx,
            diameter_m,
            length_m,
            area_m2,
        }
    }
}

pub fn default_n_cells(length_km: f64) -> usize {
    let from_length = (length_km * CELLS_PER_KM).ceil() as usize;
    from_length.clamp(MIN_CELLS, MAX_CELLS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{ConnectionKind, EquipmentSpec, Pipe};

    fn sample_pipe() -> Pipe {
        Pipe {
            id: "P1".into(),
            from: "A".into(),
            to: "B".into(),
            kind: ConnectionKind::Pipe,
            is_open: true,
            length_km: 10.0,
            diameter_mm: 600.0,
            roughness_mm: 0.012,
            compressor_ratio_max: None,
            flow_min_m3s: None,
            flow_max_m3s: None,
            equipment: EquipmentSpec::default(),
        }
    }

    #[test]
    fn mesh_geometry_from_pipe() {
        let mesh = PipeMesh::from_pipe(&sample_pipe(), Some(10));
        assert_eq!(mesh.n_cells, 10);
        assert!((mesh.length_m - 10_000.0).abs() < 1e-9);
        assert!((mesh.dx - 1_000.0).abs() < 1e-9);
        assert!((mesh.diameter_m - 0.6).abs() < 1e-9);
        assert!(mesh.area_m2 > 0.0);
    }
}
