//! Modèle intermédiaire entre importeurs et `GasNetwork`.

use super::ConnectionKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawNodeRole {
    Source,
    Sink,
    Innode,
}

impl RawNodeRole {
    pub fn from_label(label: &str) -> Self {
        match label.to_ascii_lowercase().as_str() {
            "source" | "alim" | "entry" | "production" | "poste" | "poste_alim" => Self::Source,
            "sink" | "livr" | "exit" | "consumer" | "demand" | "pdl" | "client" | "livraison" => {
                Self::Sink
            }
            "jonc" | "jonction" | "branchement" | "noeud" => Self::Innode,
            _ => Self::Innode,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RawNode {
    pub id: String,
    pub role: RawNodeRole,
    pub x: f64,
    pub y: f64,
    pub lon: Option<f64>,
    pub lat: Option<f64>,
    pub height_m: f64,
    pub pressure_lower_bar: Option<f64>,
    pub pressure_upper_bar: Option<f64>,
    pub pressure_fixed_bar: Option<f64>,
    pub flow_min_m3s: Option<f64>,
    pub flow_max_m3s: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RawPipe {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: ConnectionKind,
    pub is_open: bool,
    pub length_km: f64,
    pub diameter_mm: f64,
    pub roughness_mm: f64,
    pub compressor_ratio_max: Option<f64>,
    pub flow_min_m3s: Option<f64>,
    pub flow_max_m3s: Option<f64>,
    pub equipment: super::EquipmentSpec,
}

#[derive(Debug, Clone)]
pub struct RawNetwork {
    pub nodes: Vec<RawNode>,
    pub pipes: Vec<RawPipe>,
    pub source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::RawNodeRole;

    #[test]
    fn french_sig_role_aliases() {
        assert_eq!(RawNodeRole::from_label("PDL"), RawNodeRole::Sink);
        assert_eq!(RawNodeRole::from_label("poste_alim"), RawNodeRole::Source);
        assert_eq!(RawNodeRole::from_label("JONC"), RawNodeRole::Innode);
    }

    #[test]
    fn connection_kind_from_french_labels() {
        use super::super::ConnectionKind;
        assert_eq!(
            ConnectionKind::from_label("detendeur"),
            ConnectionKind::PressureRegulator
        );
        assert_eq!(
            ConnectionKind::from_label("poste_livraison"),
            ConnectionKind::DeliveryStation
        );
        assert_eq!(
            ConnectionKind::from_label("vanne_cv"),
            ConnectionKind::ControlValve
        );
    }
}
