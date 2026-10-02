use std::collections::HashMap;

use crate::config;

pub fn aggregate_battery_state(
    devices: &HashMap<String, (f64, String)>,
    mode: &config::BatteryMode,
) -> (f64, String) {
    if devices.is_empty() {
        return (100.0, "unknown".to_string());
    }

    // devices is a HashMap, so iteration order is arbitrary. Sort by device
    // name so `first` and the combined-mode tie-break are reproducible.
    let mut names: Vec<&String> = devices.keys().collect();
    names.sort();
    let ordered: Vec<&(f64, String)> = names.iter().map(|name| &devices[*name]).collect();

    // A NaN percentage comes from a malformed UPower payload. Dropping it beats
    // letting it win an ordering comparison via partial_cmp returning None.
    let usable: Vec<&(f64, String)> =
        ordered.iter().copied().filter(|(pct, _)| !pct.is_nan()).collect();
    if usable.is_empty() {
        return (100.0, "unknown".to_string());
    }
    let pick = &usable;

    match mode {
        config::BatteryMode::First => {
            let (pct, state) = pick[0];
            (*pct, state.clone())
        }
        config::BatteryMode::Highest => {
            let (pct, state) = pick.iter().max_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
            (*pct, state.clone())
        }
        config::BatteryMode::Lowest => {
            let (pct, state) = pick.iter().min_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
            (*pct, state.clone())
        }
        config::BatteryMode::Combined => {
            let sum: f64 = pick.iter().map(|(pct, _)| pct).sum();
            let avg = sum / pick.len() as f64;
            let any_charging = pick.iter().any(|(_, s)| s == "charging");
            let any_discharging = pick.iter().any(|(_, s)| s == "discharging");
            let state = if any_charging {
                "charging".to_string()
            } else if any_discharging {
                "discharging".to_string()
            } else {
                pick[0].1.clone()
            };
            (avg, state)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregate_first_picks_lowest_device_name() {
        let mut devices = HashMap::new();
        devices.insert("/bat1".into(), (40.0, "discharging".into()));
        devices.insert("/bat0".into(), (80.0, "discharging".into()));

        // HashMap iteration order is arbitrary, so this used to return either
        // battery depending on the run. Sorted by name it is always bat0.
        for _ in 0..50 {
            let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::First);
            assert!((pct - 80.0).abs() < 0.01);
            assert_eq!(state, "discharging");
        }
    }

    #[test]
    fn test_aggregate_ignores_nan_percentages() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (80.0, "discharging".into()));
        devices.insert("/bat1".into(), (f64::NAN, "discharging".into()));

        // A NaN from a malformed UPower payload must not win the ordering
        // comparison and get reported as a percentage.
        for mode in [
            config::BatteryMode::Highest,
            config::BatteryMode::Lowest,
            config::BatteryMode::First,
        ] {
            let (pct, _) = aggregate_battery_state(&devices, &mode);
            assert!(
                !pct.is_nan() && (pct - 80.0).abs() < 0.01,
                "mode {:?} produced {}",
                mode,
                pct
            );
        }

        let (avg, _) = aggregate_battery_state(&devices, &config::BatteryMode::Combined);
        assert!((avg - 80.0).abs() < 0.01);
    }

    #[test]
    fn test_aggregate_all_nan_falls_back_to_unknown() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (f64::NAN, "unknown".into()));
        devices.insert("/bat1".into(), (f64::NAN, "unknown".into()));

        let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::Combined);
        assert!((pct - 100.0).abs() < 0.01);
        assert_eq!(state, "unknown");
    }

    #[test]
    fn test_aggregate_highest() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (80.0, "discharging".into()));
        devices.insert("/bat1".into(), (40.0, "discharging".into()));

        let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::Highest);
        assert!((pct - 80.0).abs() < 0.01);
        assert_eq!(state, "discharging");
    }

    #[test]
    fn test_aggregate_lowest() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (80.0, "discharging".into()));
        devices.insert("/bat1".into(), (40.0, "discharging".into()));

        let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::Lowest);
        assert!((pct - 40.0).abs() < 0.01);
        assert_eq!(state, "discharging");
    }

    #[test]
    fn test_aggregate_combined() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (80.0, "charging".into()));
        devices.insert("/bat1".into(), (40.0, "discharging".into()));

        let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::Combined);
        assert!((pct - 60.0).abs() < 0.01);
        assert_eq!(state, "charging");
    }

    #[test]
    fn test_aggregate_combined_all_discharging() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (80.0, "discharging".into()));
        devices.insert("/bat1".into(), (40.0, "discharging".into()));

        let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::Combined);
        assert!((pct - 60.0).abs() < 0.01);
        assert_eq!(state, "discharging");
    }

    #[test]
    fn test_aggregate_empty() {
        let devices = HashMap::new();
        let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::Combined);
        assert!((pct - 100.0).abs() < 0.01);
        assert_eq!(state, "unknown");
    }

    #[test]
    fn test_aggregate_single_device() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (75.0, "charging".into()));

        let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::Combined);
        assert!((pct - 75.0).abs() < 0.01);
        assert_eq!(state, "charging");
    }

    #[test]
    fn test_aggregate_empty_all_modes() {
        let devices = HashMap::new();
        for mode in [
            config::BatteryMode::First,
            config::BatteryMode::Highest,
            config::BatteryMode::Lowest,
            config::BatteryMode::Combined,
        ] {
            let (pct, state) = aggregate_battery_state(&devices, &mode);
            assert!((pct - 100.0).abs() < 0.01, "Empty devices should return 100% for {:?}", mode);
            assert_eq!(state, "unknown", "Empty devices should return unknown for {:?}", mode);
        }
    }

    #[test]
    fn test_aggregate_combined_all_full() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (100.0, "full".into()));
        devices.insert("/bat1".into(), (100.0, "full".into()));

        let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::Combined);
        assert!((pct - 100.0).abs() < 0.01);
        // Neither charging nor discharging, should fall through to first device's state
        assert_eq!(state, "full");
    }

    #[test]
    fn test_aggregate_three_devices() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (90.0, "discharging".into()));
        devices.insert("/bat1".into(), (60.0, "discharging".into()));
        devices.insert("/bat2".into(), (30.0, "discharging".into()));

        let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::Combined);
        assert!((pct - 60.0).abs() < 0.01); // Average of 90, 60, 30
        assert_eq!(state, "discharging");

        let (pct_h, _) = aggregate_battery_state(&devices, &config::BatteryMode::Highest);
        assert!((pct_h - 90.0).abs() < 0.01);

        let (pct_l, _) = aggregate_battery_state(&devices, &config::BatteryMode::Lowest);
        assert!((pct_l - 30.0).abs() < 0.01);
    }

    #[test]
    fn test_aggregate_highest_equal_values() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (50.0, "discharging".into()));
        devices.insert("/bat1".into(), (50.0, "charging".into()));

        let (pct, _) = aggregate_battery_state(&devices, &config::BatteryMode::Highest);
        assert!((pct - 50.0).abs() < 0.01);
    }
}
