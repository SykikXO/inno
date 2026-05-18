use std::collections::HashMap;

use crate::config;

pub fn aggregate_battery_state(
    devices: &HashMap<String, (f64, String)>,
    mode: &config::BatteryMode,
) -> (f64, String) {
    if devices.is_empty() {
        return (100.0, "unknown".to_string());
    }

    match mode {
        config::BatteryMode::First => {
            let (_, (pct, state)) = devices.iter().next().unwrap();
            (*pct, state.clone())
        }
        config::BatteryMode::Highest => {
            let (pct, state) = devices
                .values()
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap();
            (*pct, state.clone())
        }
        config::BatteryMode::Lowest => {
            let (pct, state) = devices
                .values()
                .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap();
            (*pct, state.clone())
        }
        config::BatteryMode::Combined => {
            let sum: f64 = devices.values().map(|(pct, _)| pct).sum();
            let avg = sum / devices.len() as f64;
            let any_charging = devices.values().any(|(_, s)| s == "charging");
            let any_discharging = devices.values().any(|(_, s)| s == "discharging");
            let state = if any_charging {
                "charging".to_string()
            } else if any_discharging {
                "discharging".to_string()
            } else {
                devices.values().next().unwrap().1.clone()
            };
            (avg, state)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregate_first() {
        let mut devices = HashMap::new();
        devices.insert("/bat0".into(), (80.0, "discharging".into()));
        devices.insert("/bat1".into(), (40.0, "discharging".into()));

        let (pct, state) = aggregate_battery_state(&devices, &config::BatteryMode::First);
        assert!((pct == 80.0 || pct == 40.0) && (state == "discharging"));
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
