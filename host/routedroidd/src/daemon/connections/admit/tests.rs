use routedroid_ipc::Ipv4Net;

use super::*;

fn link(
    name: &str,
    blocks: &[([u8; 4], u8)],
    dhcp: bool,
    ineligible: Option<&str>,
) -> InterfaceInfo {
    InterfaceInfo {
        name: name.into(),
        up: true,
        addresses: vec![],
        default_route: false,
        phone_addresses: blocks
            .iter()
            .map(|&(address, prefix)| Ipv4Net {
                address: address.into(),
                prefix,
            })
            .collect(),
        dhcp,
        ineligible: ineligible.map(Into::into),
    }
}

fn host() -> Vec<InterfaceInfo> {
    vec![
        link("lan0", &[([10, 0, 0, 0], 24)], false, None),
        link("wifi0", &[], true, None),
        link("mgmt0", &[], false, Some("not in the helper policy")),
    ]
}

#[test]
fn an_allowed_start_passes() {
    assert_eq!(refusal(&host(), "lan0", Some([10, 0, 0, 7].into())), None);
    assert_eq!(refusal(&host(), "wifi0", None), None);
}

#[test]
fn a_link_outside_the_policy_says_where_to_allow_it() {
    assert_eq!(
        refusal(&host(), "mgmt0", None).unwrap(),
        "mgmt0: not in the helper policy (allow it in /etc/routedroid/helper.toml)"
    );
    assert_eq!(
        refusal(&host(), "eth9", None).unwrap(),
        "eth9: no such interface"
    );
}

#[test]
fn an_address_outside_the_blocks_names_them() {
    assert_eq!(
        refusal(&host(), "lan0", Some([10, 0, 1, 7].into())).unwrap(),
        "10.0.1.7 is not a phone address the policy allows on lan0 (it allows 10.0.0.0/24)"
    );
    assert_eq!(
        refusal(&host(), "wifi0", Some([10, 0, 0, 7].into())).unwrap(),
        "10.0.0.7 is not a phone address the policy allows on wifi0 (it allows DHCP only)"
    );
}

#[test]
fn dhcp_where_only_blocks_are_allowed_asks_for_an_address() {
    assert_eq!(
        refusal(&host(), "lan0", None).unwrap(),
        "the policy does not allow DHCP on lan0: give the phone an address with --phone-ip (it allows 10.0.0.0/24)"
    );
}

#[test]
fn a_zero_prefix_holds_everything() {
    let any = vec![link("lan0", &[([0, 0, 0, 0], 0)], false, None)];
    assert_eq!(refusal(&any, "lan0", Some([192, 168, 1, 1].into())), None);
}
