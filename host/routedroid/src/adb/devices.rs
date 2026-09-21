//! `adb devices -l` parsing.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    Device,
    Unauthorized,
    Offline,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub state: DeviceState,
    /// `model:` from the `-l` columns, when present.
    pub model: Option<String>,
    pub transport: super::Transport,
}

pub fn parse_devices(text: &str) -> Vec<Device> {
    text.lines()
        .skip_while(|l| !l.starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let serial = it.next()?.to_string();
            let state = match it.next()? {
                "device" => DeviceState::Device,
                "unauthorized" => DeviceState::Unauthorized,
                "offline" => DeviceState::Offline,
                other => DeviceState::Other(other.to_string()),
            };
            let model = it.find_map(|kv| kv.strip_prefix("model:").map(str::to_string));
            let transport = super::Transport::classify(&serial);
            Some(Device { serial, state, model, transport })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_long_listing() {
        let text = "List of devices attached\n\
                    R58M12345AB            device usb:1-2 product:j8y18lte model:SM_J810G device:j8y18lte transport_id:3\n\
                    emulator-5554          device product:sdk_gphone64_x86_64 model:sdk_gphone64_x86_64 device:emu64xa transport_id:5\n\
                    0123456789             unauthorized transport_id:7\n\n";
        let d = parse_devices(text);
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].serial, "R58M12345AB");
        assert_eq!(d[0].state, DeviceState::Device);
        assert_eq!(d[0].model.as_deref(), Some("SM_J810G"));
        assert_eq!(d[0].transport, super::super::Transport::Usb);
        assert_eq!(d[1].transport, super::super::Transport::Emulator);
        assert_eq!(d[2].state, DeviceState::Unauthorized);
        assert!(parse_devices("").is_empty());
    }
}
