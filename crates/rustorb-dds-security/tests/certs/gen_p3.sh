#!/bin/sh
# Makes participant3 (EC P-256) from the files of gen.sh (run that
# first): a third participant for the tests of identity revocation and
# key revision, with the permissions of participant2.
set -e
cd "$(dirname "$0")"
days=36500
openssl ecparam -name prime256v1 -genkey -noout | openssl pkcs8 -topk8 -nocrypt -out p3_key.pem
openssl req -new -key p3_key.pem -subj "/C=JP/O=rustorb/CN=participant3" -out p3.csr
openssl x509 -req -in p3.csr -CA identity_ca.pem -CAkey ca_key.pem -CAcreateserial -sha256 -days $days -out p3_cert.pem
rm -f p3.csr *.srl
sed -e 's/grant name="p2"/grant name="p3"/' -e 's/CN=participant2/CN=participant3/' permissions_p2.xml > permissions_p3.xml
openssl smime -sign -in permissions_p3.xml -text -out permissions_p3.p7s -signer permissions_ca.pem -inkey perm_ca_key.pem
