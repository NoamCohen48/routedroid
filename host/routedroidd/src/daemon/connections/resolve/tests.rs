use std::net::Ipv4Addr;

use routedroid_ipc::DnsChoice;

use super::*;

fn device(serial: &str, state: DeviceState) -> Device {
    Device {
        serial: serial.into(),
        state,
        model: None,
    }
}

fn link(name: &str, ineligible: Option<&str>) -> InterfaceInfo {
    InterfaceInfo {
        name: name.into(),
        up: true,
        addresses: vec![],
        default_route: false,
        phone_addresses: vec![],
        dhcp: true,
        ineligible: ineligible.map(Into::into),
    }
}

fn pixel() -> Phone {
    Phone {
        serial: "R58".into(),
        name: Some("pixel".into()),
        lan_if: Some("wifi0".into()),
        phone_ip: Some(Ipv4Addr::new(10, 0, 0, 7)),
        dns: Some(DnsChoice::None),
        ..Phone::default()
    }
}

fn bare() -> StartRequest {
    StartRequest {
        serial: None,
        lan_if: None,
        phone_ip: None,
        tun: None,
        mtu: None,
        dns: None,
        connect_timeout_secs: None,
        reconnect_secs: None,
        allow_network_adb: false,
    }
}

struct World {
    phones: Vec<Phone>,
    attached: Vec<Device>,
    connected: Vec<String>,
    interfaces: Option<Vec<InterfaceInfo>>,
}

impl World {
    fn new() -> Self {
        Self {
            phones: vec![pixel()],
            attached: vec![device("R58", DeviceState::Device)],
            connected: vec![],
            interfaces: Some(vec![
                link("lan0", None),
                link("mgmt0", Some("not in the helper policy")),
            ]),
        }
    }

    fn resolve(&self, request: StartRequest) -> std::result::Result<StartRequest, String> {
        let known = Known {
            phones: &self.phones,
            attached: &self.attached,
            connected: &self.connected,
            interfaces: self.interfaces.as_deref(),
        };
        resolve(request, &known).map_err(|e| e.to_string())
    }
}

#[test]
fn a_bare_start_takes_the_one_phone_and_its_remembered_options() {
    let request = World::new().resolve(bare()).unwrap();
    assert_eq!(request.serial.as_deref(), Some("R58"));
    assert_eq!(request.lan_if.as_deref(), Some("wifi0"));
    assert_eq!(request.phone_ip, Some(Ipv4Addr::new(10, 0, 0, 7)));
    assert_eq!(request.dns, Some(DnsChoice::None));
}

#[test]
fn a_name_is_its_serial_and_what_the_client_says_wins() {
    let asked = StartRequest {
        serial: Some("pixel".into()),
        lan_if: Some("lan0".into()),
        dns: Some(DnsChoice::Auto),
        ..bare()
    };
    let request = World::new().resolve(asked).unwrap();
    assert_eq!(request.serial.as_deref(), Some("R58"));
    assert_eq!(request.lan_if.as_deref(), Some("lan0"));
    assert_eq!(request.phone_ip, None, "the remembered address is wifi0's");
    assert_eq!(request.dns, Some(DnsChoice::Auto));
}

#[test]
fn an_unknown_phone_gets_the_one_lan_the_policy_allows() {
    let mut world = World::new();
    world.attached = vec![device("ZX1", DeviceState::Device)];
    let request = world.resolve(bare()).unwrap();
    assert_eq!(request.serial.as_deref(), Some("ZX1"));
    assert_eq!(request.lan_if.as_deref(), Some("lan0"));
}

#[test]
fn the_phone_not_yet_connected_is_the_one() {
    let mut world = World::new();
    world.attached.push(device("ZX1", DeviceState::Device));
    world
        .attached
        .push(device("AB2", DeviceState::Unauthorized));
    assert_eq!(
        world.resolve(bare()).unwrap_err(),
        "2 phones attached (R58, ZX1): say which one"
    );
    world.connected = vec!["R58".into()];
    assert_eq!(
        world.resolve(bare()).unwrap().serial.as_deref(),
        Some("ZX1")
    );
}

#[test]
fn nothing_to_pick_says_what_to_do() {
    let mut world = World::new();
    world.attached.clear();
    assert!(
        world
            .resolve(bare())
            .unwrap_err()
            .starts_with("no phone attached")
    );
    world.attached = vec![device("ZX1", DeviceState::Unauthorized)];
    assert!(
        world
            .resolve(bare())
            .unwrap_err()
            .starts_with("no phone ready")
    );
    world.attached = vec![device("ZX1", DeviceState::Device)];
    world.interfaces = Some(vec![link("mgmt0", Some("not in the helper policy"))]);
    assert_eq!(
        world.resolve(bare()).unwrap_err(),
        "no interface may carry phones yet: run `sudo routedroid setup`"
    );
    world.interfaces = Some(vec![link("lan0", None), link("wifi0", None)]);
    assert_eq!(
        world.resolve(bare()).unwrap_err(),
        "phones may join through lan0, wifi0: say which with --lan-if"
    );
    world.interfaces = None;
    assert!(world.resolve(bare()).unwrap_err().contains("--lan-if"));
}
