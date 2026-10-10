# Firewalls

Routedroid adds one nftables table per connection (`inet routedroid_phoneN`), with
policy-accept chains that filter only that phone's traffic. An accept there cannot override
a drop in your own firewall.

ufw, as Ubuntu ships it, is the common case. Ping still reaches the phone, but TCP and UDP
to and from it are dropped, DNS included.

## What doctor says

`routedroid doctor` finds IPv4 forward chains that drop by default, and prints the command
that lets the phone interfaces through, in the firewall's own terms:

```sh
# ufw (tested)
sudo ufw route allow in on phone+ && sudo ufw route allow out on phone+

# iptables
sudo iptables -I FORWARD -i phone+ -j ACCEPT && sudo iptables -I FORWARD -o phone+ -j ACCEPT

# your own nftables table
sudo nft insert rule inet filter forward iifname "phone*" accept && \
    sudo nft insert rule inet filter forward oifname "phone*" accept
```

Once the firewall lets `phone*` through both ways, doctor stops warning.

## firewalld

Put each TUN in a zone that forwards to and from your LAN's zone, for example:

```sh
firewall-cmd --zone=trusted --add-interface=phone0
```

Then check from another LAN host.

## What stays limited

Opening the forward chain for `phone*` does not open the phone to everything. Routedroid's
own table still limits each phone to its address and its LAN.
