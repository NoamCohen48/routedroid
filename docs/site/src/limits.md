# Known limits

- **IPv4 only.** The phone has no IPv6 while connected.
- **One LAN per phone.** Android picks one source address, so a second LAN could not be
  answered correctly.
- **Not an Ethernet bridge.** No broadcast or multicast discovery (mDNS, SSDP) reaches the
  phone.
- **Some networks refuse it.** Networks that allow one address per MAC on a port (strict
  DHCP snooping, 802.1X with a single host, some captive portals), and networks that block
  proxy ARP, do not work. `--phone-ip` does not help there either.
- **It is a VPN on the phone.** The phone shows that a VPN is active, and another VPN app on
  it takes over the connection.
- **USB only, in practice.** Network adb serials (`host:port`) need `--allow-network-adb` and
  are untested. The VPN's default route may cut adb itself.
- **Linux with systemd** on the PC.
