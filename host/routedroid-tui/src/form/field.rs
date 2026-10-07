//! The form's fields, in Tab order.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    LanIf,
    PhoneIp,
    Dns,
    Mtu,
    Tun,
    Timeout,
    ReconnectWait,
    Name,
    Remember,
    NetworkAdb,
}

impl Field {
    pub const ALL: [Field; 10] = [
        Field::LanIf,
        Field::PhoneIp,
        Field::Dns,
        Field::Mtu,
        Field::Tun,
        Field::Timeout,
        Field::ReconnectWait,
        Field::Name,
        Field::Remember,
        Field::NetworkAdb,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::LanIf => "LAN interface (empty: the one allowed)",
            Field::PhoneIp => "Phone IP (empty: DHCP)",
            Field::Dns => "DNS (empty: automatic, \"none\", or a list)",
            Field::Mtu => "MTU (empty: default)",
            Field::Tun => "TUN name (empty: next phoneN)",
            Field::Timeout => "App connect timeout (e.g. 90s, 2m)",
            Field::ReconnectWait => "Wait for an unplugged phone (e.g. 5m)",
            Field::Name => "Name (remembers the phone)",
            Field::Remember => "Remember: connect when plugged in (Space)",
            Field::NetworkAdb => "Allow network ADB (Space toggles)",
        }
    }

    pub(super) fn index(self) -> usize {
        Field::ALL
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0)
    }
}
