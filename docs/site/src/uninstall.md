# Uninstall

```sh
sudo apt remove routedroid                  # or dnf remove; apt purge also drops the policy, state and group
sudo host/install.sh --uninstall            # a source or tarball install
sudo host/install.sh --uninstall --purge    # also the policy, /var/lib/routedroid and the group
adb uninstall dev.routedroid                # the app, on each phone
```

Removal ends every connection first. It replays the journals and removes what was left
behind, even with phones connected. Afterwards no TUN, route, rule, nftables table or sysctl
change of Routedroid's remains.

A purge keeps the state directory and the group if anything could not be undone.

Your daemon keeps running until:

```sh
systemctl --user disable --now routedroid
```

Remembered phones stay in `~/.config/routedroid/phones.toml` until you delete it.

## Upgrades

An upgrade leaves connections up. Running ones keep their helper, and the next one starts
the new version. Restart your daemon to run the new one:

```sh
systemctl --user restart routedroid
```
