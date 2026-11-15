#!/bin/sh
# Makes the files of the tests of encrypted private keys and CRLs from
# the files of gen.sh (run that first): participant1's key encrypted
# three ways (password "OpenSesame"), participant2's RSA key encrypted,
# and CRLs of the identity CA.
set -e
cd "$(dirname "$0")"
pass=OpenSesame
# PKCS #8, PBES2 (PBKDF2 and AES-256-CBC).
openssl pkcs8 -topk8 -in p1_key.pem -v2 aes-256-cbc -passout pass:$pass -out p1_key_pkcs8_enc.pem
# PEM with DEK-Info, the key derived from the passphrase.
openssl ec -in p1_key.pem -aes256 -passout pass:$pass -out p1_key_dek.pem 2>/dev/null
openssl rsa -in p2_key.pem -traditional -aes128 -passout pass:$pass -out p2_key_dek.pem 2>/dev/null
# PEM with DEK-Info AES-128-CBC and the key itself given in Base64, as
# DDS Security 1.2, Table 50, words the password.
key=000102030405060708090a0b0c0d0e0f
iv=f0e0d0c0b0a090807060504030201000
openssl ec -in p1_key.pem -outform DER 2>/dev/null |
  openssl enc -aes-128-cbc -K $key -iv $iv | openssl base64 > body.b64
{
  echo "-----BEGIN EC PRIVATE KEY-----"
  echo "Proc-Type: 4,ENCRYPTED"
  echo "DEK-Info: AES-128-CBC,$(echo $iv | tr a-f A-F)"
  echo
  cat body.b64
  echo "-----END EC PRIVATE KEY-----"
} > p1_key_raw.pem
rm -f body.b64
# A CRL that revokes participant2, one that has expired, and one signed
# by another CA.
mkdir -p ca.tmp
: > ca.tmp/index.txt
echo 1000 > ca.tmp/crlnumber
cat > ca.tmp/ca.cnf <<CNF
[ ca ]
default_ca = test
[ test ]
database = ca.tmp/index.txt
crlnumber = ca.tmp/crlnumber
default_md = sha256
default_crl_days = 36500
CNF
openssl ca -config ca.tmp/ca.cnf -cert identity_ca.pem -keyfile ca_key.pem -revoke p2_cert.pem 2>/dev/null
openssl ca -config ca.tmp/ca.cnf -cert identity_ca.pem -keyfile ca_key.pem -gencrl -out crl_p2.pem 2>/dev/null
openssl ca -config ca.tmp/ca.cnf -cert identity_ca.pem -keyfile ca_key.pem -gencrl -crlsec 1 -out crl_expired.pem 2>/dev/null
openssl ca -config ca.tmp/ca.cnf -cert permissions_ca.pem -keyfile perm_ca_key.pem -gencrl -out crl_other_ca.pem 2>/dev/null
rm -rf ca.tmp
