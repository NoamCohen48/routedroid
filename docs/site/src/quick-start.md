# Quick start

This assumes Routedroid is [installed](install.md) and [set up](setup.md), and you have
logged in again since.

1. **Plug the phone in** by USB, with USB debugging on. Accept "Allow USB debugging?" on
   the phone the first time.

2. **Check that Routedroid sees it:**

   ```sh
   routedroid devices
   ```

3. **Connect it:**

   ```sh
   routedroid start
   ```

   With one phone attached and one interface allowed, that is all it needs. It prints where
   the phone joins, then follows the connection: `starting`, `waiting for app`,
   `handshaking`, `active`.

4. **Answer the phone.** The first time, the phone asks for VPN permission. Unlock it and
   allow it. While connected, the phone shows its VPN key icon and a notification with
   Stop.

5. **Use it.** From another machine on the LAN, the phone answers at its address:

   ```sh
   routedroid status        # in another terminal: the address, the lease, traffic
   ping 192.168.1.57        # from another LAN host, with the address status showed
   ```

6. **Disconnect** with Ctrl-C, or `routedroid stop` from another terminal.

## Keep it connected

`--detach` returns as soon as the daemon has accepted the start. The connection belongs to
the daemon, not to the command that started it:

```sh
routedroid start --detach
routedroid stop
```

To have the phone connect by itself whenever it is plugged in, give it a name and remember
it:

```sh
routedroid start --name pixel --remember
```

See [Remembered phones](remembered-phones.md).

## If something goes wrong

Run `routedroid doctor`, then see [Troubleshooting](troubleshooting.md).
