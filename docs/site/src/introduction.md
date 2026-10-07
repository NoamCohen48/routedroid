# Routedroid

Routedroid makes an Android phone plugged into a Linux PC a real host on the PC's LAN. The
phone gets its own IPv4 address there, leased from the LAN's DHCP server or chosen by you.
Other machines on the LAN can reach it, and its traffic leaves through that LAN.

Nothing on the phone is rooted, and nothing on the LAN changes. You need USB debugging on
the phone, the Routedroid app, and a Linux PC with systemd.

<svg viewBox="0 0 760 170" width="100%" role="img" aria-label="A LAN host reaches the phone through the PC's LAN interface, a TUN device, adb and the phone's VpnService" style="max-width:760px;font-family:sans-serif;font-size:13px">
  <style>
    .box { fill: none; stroke: currentColor; stroke-width: 1.5; rx: 6; }
    .line { stroke: currentColor; stroke-width: 1.5; }
    .note { font-size: 11px; opacity: 0.75; }
  </style>
  <rect class="box" x="10" y="40" width="110" height="50"/>
  <text x="65" y="70" text-anchor="middle" fill="currentColor">LAN host</text>
  <line class="line" x1="120" y1="65" x2="170" y2="65"/>
  <text x="145" y="58" text-anchor="middle" class="note" fill="currentColor">LAN</text>
  <rect class="box" x="170" y="20" width="300" height="90" style="stroke-dasharray:4 3"/>
  <text x="320" y="135" text-anchor="middle" class="note" fill="currentColor">the Linux PC</text>
  <rect class="box" x="185" y="40" width="120" height="50"/>
  <text x="245" y="62" text-anchor="middle" fill="currentColor">eno1</text>
  <text x="245" y="78" text-anchor="middle" class="note" fill="currentColor">proxy ARP</text>
  <line class="line" x1="305" y1="65" x2="335" y2="65"/>
  <rect class="box" x="335" y="40" width="120" height="50"/>
  <text x="395" y="62" text-anchor="middle" fill="currentColor">phone0</text>
  <text x="395" y="78" text-anchor="middle" class="note" fill="currentColor">TUN, /32 route</text>
  <line class="line" x1="470" y1="65" x2="530" y2="65"/>
  <text x="500" y="58" text-anchor="middle" class="note" fill="currentColor">USB, adb</text>
  <rect class="box" x="530" y="20" width="220" height="90" style="stroke-dasharray:4 3"/>
  <text x="640" y="135" text-anchor="middle" class="note" fill="currentColor">the phone</text>
  <rect class="box" x="545" y="40" width="190" height="50"/>
  <text x="640" y="62" text-anchor="middle" fill="currentColor">VpnService</text>
  <text x="640" y="78" text-anchor="middle" class="note" fill="currentColor">apps on the phone</text>
</svg>

Packets travel as raw IPv4 over `adb reverse` into an Android `VpnService`. On the PC, a TUN
device, a `/32` route and proxy ARP on the LAN interface make the phone's address answer.
Everything the PC changes is made by a small root helper and undone when the connection ends,
even after a crash.

## What you get

- **A real address.** Other machines reach the phone at its own address: SSH into Termux,
  test an app's server, or put the phone on a lab network without Wi-Fi.
- **Its traffic leaves through that LAN**, even when the PC's default route goes somewhere
  else, such as a VPN.
- **No NAT.** A capture on the LAN shows the phone's own address.
- **Unplugging is fine.** A phone that goes away keeps its address for a while and resumes
  when it is back.
- **Several phones at once**, each with its own address.
- **A CLI, a TUI and a JSON API** for scripts.

## Where to go next

- [Install](install.md), then [Set up](setup.md): one `sudo routedroid setup`.
- [Quick start](quick-start.md): the first connection.
- [How it works](how-it-works.md) and the [security model](security.md), to know what runs
  as root.

The source lives at <https://github.com/NoamCohen48/routedroid>.
