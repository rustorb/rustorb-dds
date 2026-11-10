#!/bin/sh
# Makes the test certificates: an identity CA (EC P-256), an EC and an
# RSA participant, a permissions CA. Valid for 100 years.
set -e
cd "$(dirname "$0")"
days=36500
openssl ecparam -name prime256v1 -genkey -noout -out ca_key.pem
openssl req -x509 -new -key ca_key.pem -sha256 -days $days -subj "/C=JP/O=rustorb/CN=rustorb test identity CA" -out identity_ca.pem
openssl ecparam -name prime256v1 -genkey -noout | openssl pkcs8 -topk8 -nocrypt -out p1_key.pem
openssl req -new -key p1_key.pem -subj "/C=JP/O=rustorb/CN=participant1" -out p1.csr
openssl x509 -req -in p1.csr -CA identity_ca.pem -CAkey ca_key.pem -CAcreateserial -sha256 -days $days -out p1_cert.pem
openssl genrsa -traditional -out p2_key.pem 2048 2>/dev/null || openssl genrsa -out p2_key.pem 2048
openssl req -new -key p2_key.pem -subj "/C=JP/O=rustorb/CN=participant2" -out p2.csr
openssl x509 -req -in p2.csr -CA identity_ca.pem -CAkey ca_key.pem -CAcreateserial -sha256 -days $days -out p2_cert.pem
openssl genrsa -out perm_ca_key.pem 2048
openssl req -x509 -new -key perm_ca_key.pem -sha256 -days $days -subj "/C=JP/O=rustorb/CN=rustorb test permissions CA" -out permissions_ca.pem
rm -f *.csr *.srl
