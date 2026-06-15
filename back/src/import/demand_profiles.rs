//! Import CSV de profils de demande par nœud (P9.4).

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::solver::demand::{ClientCategory, DemandProfile};

/// Parse un CSV : `node_id,category,q0_m3h,alpha_m3h_per_c,t_threshold_c[,max_heating_m3h]`.
pub fn parse_demand_profiles_csv(content: &str) -> Result<HashMap<String, DemandProfile>> {
    let mut reader = csv::Reader::from_reader(content.as_bytes());
    let headers = reader.headers()?.clone();
    let node_col = find_column(&headers, &["node_id", "node", "id"])?;
    let cat_col = find_column(&headers, &["category", "categorie", "type"])?;
    let q0_col = find_column(&headers, &["q0_m3h", "q0", "base_m3h"])?;
    let alpha_col = find_column(&headers, &["alpha_m3h_per_c", "alpha", "gradient"])?;
    let thresh_col = find_column(&headers, &["t_threshold_c", "t_seuil", "threshold_c"])?;
    let max_heat_col = find_optional_column(&headers, &["max_heating_m3h", "q_chauff_max"])?;

    let mut profiles = HashMap::new();
    for row in reader.records() {
        let row = row?;
        let node_id = field(&row, node_col, "node_id")?;
        if profiles.contains_key(&node_id) {
            bail!("duplicate node_id in demand profiles: {node_id}");
        }
        let category = parse_category(field(&row, cat_col, "category")?.as_str())?;
        let q0: f64 = field(&row, q0_col, "q0_m3h")?.parse().context("q0_m3h")?;
        let alpha: f64 = field(&row, alpha_col, "alpha")?.parse().context("alpha")?;
        let threshold: f64 = field(&row, thresh_col, "t_threshold_c")?
            .parse()
            .context("t_threshold_c")?;

        let mut profile = if let Some(cat) = category {
            let mut p = DemandProfile::from_category(cat);
            p.q0_m3h = q0;
            p.alpha_m3h_per_c = alpha;
            p.t_threshold_c = threshold;
            p
        } else {
            DemandProfile::new(q0, alpha, threshold)
        };
        profile.category = category;
        if let Some(idx) = max_heat_col {
            if let Some(raw) = optional_field(&row, idx) {
                profile.max_heating_m3h = Some(raw.parse().context("max_heating_m3h")?);
            }
        }
        profiles.insert(node_id, profile);
    }
    Ok(profiles)
}

pub fn load_demand_profiles_csv(path: &Path) -> Result<HashMap<String, DemandProfile>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("open demand profiles csv: {}", path.display()))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    parse_demand_profiles_csv(&content)
}

fn find_column(headers: &csv::StringRecord, aliases: &[&str]) -> Result<usize> {
    find_optional_column(headers, aliases)?.ok_or_else(|| {
        anyhow::anyhow!(
            "missing CSV column (expected one of: {})",
            aliases.join(", ")
        )
    })
}

fn find_optional_column(headers: &csv::StringRecord, aliases: &[&str]) -> Result<Option<usize>> {
    for (idx, header) in headers.iter().enumerate() {
        let h = header.trim().to_ascii_lowercase();
        if aliases.iter().any(|a| h == *a) {
            return Ok(Some(idx));
        }
    }
    Ok(None)
}

fn field(record: &csv::StringRecord, idx: usize, name: &str) -> Result<String> {
    record
        .get(idx)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing field {name}"))
}

fn optional_field(record: &csv::StringRecord, idx: usize) -> Option<String> {
    record
        .get(idx)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_category(raw: &str) -> Result<Option<ClientCategory>> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "residential" | "residentiel" | "résidentiel" | "domestic" | "pl_residentiel"
        | "pl_res" | "res_pl" => Ok(Some(ClientCategory::Residential)),
        "tertiary" | "tertiaire" | "pl_tertiaire" | "pl_ter" | "ter_pl" => {
            Ok(Some(ClientCategory::Tertiary))
        }
        "industrial" | "industriel" | "pl_industriel" | "pl_ind" | "ind_pl" => {
            Ok(Some(ClientCategory::Industrial))
        }
        // « pl » / « livraison » seuls : type client indéterminé → pas de preset catégorie.
        "" | "custom" | "other" | "pl" | "point_livraison" | "livraison" | "pdl" => Ok(None),
        unknown => bail!("unknown demand profile category: {unknown}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::test_corpus_root;

    #[test]
    fn test_load_demand_profiles_corpus_csv() {
        let path = test_corpus_root().join("synthetic/demand/node-profiles.csv");
        if !path.exists() {
            eprintln!("skip: {}", path.display());
            return;
        }
        let profiles = load_demand_profiles_csv(&path).expect("csv");
        assert!(profiles.contains_key("LVR01"));
        let p = &profiles["LVR01"];
        assert!(p.q0_m3h > 0.0);
        assert!(p.alpha_m3h_per_c > 0.0);
        assert_eq!(p.max_heating_m3h, Some(220.0));
    }

    #[test]
    fn test_category_preset_keeps_max_heating_without_csv_column() {
        let csv = "node_id,category,q0_m3h,alpha_m3h_per_c,t_threshold_c\nPL1,tertiary,50,2.5,17\n";
        let profiles = parse_demand_profiles_csv(csv).expect("parse");
        assert_eq!(profiles["PL1"].max_heating_m3h, Some(120.0));
    }

    #[test]
    fn test_parse_inline_csv() {
        let csv = "node_id,category,q0_m3h,alpha_m3h_per_c,t_threshold_c\nSK,residential,10,5,17\n";
        let profiles = parse_demand_profiles_csv(csv).expect("parse");
        assert_eq!(profiles.len(), 1);
        assert!((profiles["SK"].q0_m3h - 10.0).abs() < 1e-9);
        assert_eq!(profiles["SK"].max_heating_m3h, Some(220.0));
    }

    #[test]
    fn test_generic_pl_category_does_not_assume_residential() {
        let csv = "node_id,category,q0_m3h,alpha_m3h_per_c,t_threshold_c\nPL1,pl,50,2.5,17\n";
        let profiles = parse_demand_profiles_csv(csv).expect("parse");
        let p = &profiles["PL1"];
        assert_eq!(p.category, None);
        assert_eq!(p.max_heating_m3h, None);
        assert!((p.q0_m3h - 50.0).abs() < 1e-9);
    }

    #[test]
    fn test_pl_residentiel_alias() {
        let csv =
            "node_id,category,q0_m3h,alpha_m3h_per_c,t_threshold_c\nPL1,pl_residentiel,50,2.5,17\n";
        let profiles = parse_demand_profiles_csv(csv).expect("parse");
        assert_eq!(profiles["PL1"].category, Some(ClientCategory::Residential));
        assert_eq!(profiles["PL1"].max_heating_m3h, Some(220.0));
    }

    #[test]
    fn test_unknown_category_is_rejected() {
        let csv =
            "node_id,category,q0_m3h,alpha_m3h_per_c,t_threshold_c\nPL1,residensial,50,2.5,17\n";
        let err = parse_demand_profiles_csv(csv).unwrap_err();
        assert!(err.to_string().contains("unknown demand profile category"));
    }

    #[test]
    fn test_blank_max_heating_cell_uses_category_preset() {
        let csv = "node_id,category,q0_m3h,alpha_m3h_per_c,t_threshold_c,max_heating_m3h\n\
PL1,residential,50,2.5,17,\nPL2,tertiary,60,3,17,90\n";
        let profiles = parse_demand_profiles_csv(csv).expect("parse");
        assert_eq!(profiles["PL1"].max_heating_m3h, Some(220.0));
        assert_eq!(profiles["PL2"].max_heating_m3h, Some(90.0));
    }
}
