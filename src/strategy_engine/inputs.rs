//! Setelan script yang boleh diubah pemakai (seperti `input()` di Pine Script). Pembuat strategi
//! mendeklarasikannya di versi (`strategy_versions.inputs`), pemakai mengisi nilainya
//! (`strategy_subscriptions.input_values`), script membacanya lewat `inputs.<nama>`.

use rhai::{Dynamic, Map};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value};

pub const MAX_INPUTS: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSpec {
    /// Nama di script (`inputs.rsi_period`) — huruf kecil, angka, `_`, diawali huruf.
    pub name: String,
    /// Label untuk FE, mis. "Periode RSI".
    #[serde(default)]
    pub label: String,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    /// `true` = harus bilangan bulat (mis. periode).
    #[serde(default)]
    pub integer: bool,
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && name.len() <= 40
}

/// Validasi deklarasi `inputs` 1 versi. `Value::Null` = tanpa setelan.
pub fn parse_specs(raw: &Value) -> Result<Vec<InputSpec>, String> {
    if raw.is_null() {
        return Ok(Vec::new());
    }
    let specs: Vec<InputSpec> = serde_json::from_value(raw.clone())
        .map_err(|e| format!("inputs tidak valid: {e}"))?;
    if specs.len() > MAX_INPUTS {
        return Err(format!("inputs maksimal {MAX_INPUTS}"));
    }
    for (i, spec) in specs.iter().enumerate() {
        if !valid_name(&spec.name) {
            return Err(format!(
                "nama input '{}' tidak valid (huruf kecil/angka/_, diawali huruf, maks 40)",
                spec.name
            ));
        }
        if specs[..i].iter().any(|other| other.name == spec.name) {
            return Err(format!("nama input '{}' dobel", spec.name));
        }
        if !(spec.min <= spec.default && spec.default <= spec.max) {
            return Err(format!("input '{}': harus min ≤ default ≤ max", spec.name));
        }
        if spec.integer && [spec.default, spec.min, spec.max].iter().any(|v| v.fract() != 0.0) {
            return Err(format!("input '{}': default/min/max harus bilangan bulat", spec.name));
        }
    }
    Ok(specs)
}

/// Nilai setelan pilihan pemakai, dilengkapi default & dicek batasnya. Balik JSON (untuk disimpan)
/// — key yang tidak dideklarasikan ditolak.
pub fn resolve_values(specs: &[InputSpec], values: &Value) -> Result<Value, String> {
    let empty = JsonMap::new();
    let given = match values {
        Value::Null => &empty,
        Value::Object(map) => map,
        _ => return Err("input_values harus berupa object".to_string()),
    };
    if let Some(unknown) = given.keys().find(|k| !specs.iter().any(|s| &s.name == *k)) {
        return Err(format!("input '{unknown}' tidak ada di strategi ini"));
    }
    let mut resolved = JsonMap::new();
    for spec in specs {
        let value = match given.get(&spec.name) {
            None | Some(Value::Null) => spec.default,
            Some(v) => v.as_f64().ok_or_else(|| format!("input '{}' harus angka", spec.name))?,
        };
        if value < spec.min || value > spec.max {
            return Err(format!("input '{}' harus antara {} dan {}", spec.name, spec.min, spec.max));
        }
        if spec.integer && value.fract() != 0.0 {
            return Err(format!("input '{}' harus bilangan bulat", spec.name));
        }
        let json = if spec.integer { Value::from(value as i64) } else { Value::from(value) };
        resolved.insert(spec.name.clone(), json);
    }
    Ok(Value::Object(resolved))
}

/// JSON nilai setelan (hasil `resolve_values`) -> `inputs` di script.
pub(super) fn to_script_map(values: &Value) -> Map {
    values
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| {
            let value = match v.as_i64() {
                Some(i) => Dynamic::from_int(i),
                None => Dynamic::from_float(v.as_f64().unwrap_or(f64::NAN)),
            };
            (k.as_str().into(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn specs() -> Vec<InputSpec> {
        parse_specs(&json!([
            {"name": "rsi_period", "label": "Periode RSI", "default": 14, "min": 2, "max": 100, "integer": true},
            {"name": "threshold", "default": 30, "min": 1, "max": 99}
        ]))
        .unwrap()
    }

    #[test]
    fn deklarasi_input() {
        assert_eq!(specs().len(), 2);
        assert!(parse_specs(&json!([{"name": "RSI", "default": 1, "min": 0, "max": 2}])).is_err());
        assert!(parse_specs(&json!([{"name": "a", "default": 5, "min": 6, "max": 9}])).is_err());
        assert!(parse_specs(&json!([
            {"name": "a", "default": 1, "min": 0, "max": 2},
            {"name": "a", "default": 1, "min": 0, "max": 2}
        ]))
        .is_err());
    }

    #[test]
    fn nilai_input_dilengkapi_default() {
        let values = resolve_values(&specs(), &json!({"rsi_period": 10})).unwrap();
        assert_eq!(values, json!({"rsi_period": 10, "threshold": 30.0}));
        assert!(resolve_values(&specs(), &json!({"rsi_period": 1})).is_err());
        assert!(resolve_values(&specs(), &json!({"rsi_period": 10.5})).is_err());
        assert!(resolve_values(&specs(), &json!({"lain": 1})).is_err());
    }
}
