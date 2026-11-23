#!/bin/sh
# The governance of the tests of PSK protection (rustorb-dds#84): the one
# of gen.sh with rtps_psk_protection_kind ENCRYPT, signed by the
# permissions CA of gen.sh.
set -e
cd "$(dirname "$0")"
openssl smime -sign -in governance_psk.xml -text -out governance_psk.p7s -signer permissions_ca.pem -inkey perm_ca_key.pem
