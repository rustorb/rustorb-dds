#!/bin/sh
# Makes the OCSP responses of the tests from the files of gen.sh and
# gen_p3.sh (run those first): participant1 and participant2 good,
# participant3 revoked (and good, before), signed by the identity CA or
# by a responder it delegated OCSP signing to; valid for 100 years.
set -e
cd "$(dirname "$0")"
days=36500
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
openssl ecparam -name prime256v1 -genkey -noout | openssl pkcs8 -topk8 -nocrypt -out ocsp_signer_key.pem
openssl req -new -key ocsp_signer_key.pem -subj "/C=JP/O=rustorb/CN=rustorb test OCSP responder" -out "$t/signer.csr"
printf 'extendedKeyUsage=OCSPSigning\n' > "$t/ext"
openssl x509 -req -in "$t/signer.csr" -CA identity_ca.pem -CAkey ca_key.pem -CAcreateserial -sha256 -days $days -extfile "$t/ext" -out ocsp_signer_cert.pem
rm -f *.srl
serial() { openssl x509 -in "$1" -noout -serial | cut -d= -f2; }
subject() { openssl x509 -in "$1" -noout -subject -nameopt compat | sed 's/^subject=//'; }
{
  printf 'V\t21240101000000Z\t\t%s\tunknown\t%s\n' "$(serial p1_cert.pem)" "$(subject p1_cert.pem)"
  printf 'V\t21240101000000Z\t\t%s\tunknown\t%s\n' "$(serial p2_cert.pem)" "$(subject p2_cert.pem)"
  printf 'R\t21240101000000Z\t260101000000Z\t%s\tunknown\t%s\n' "$(serial p3_cert.pem)" "$(subject p3_cert.pem)"
} > "$t/index.txt"
respond() { # who, out, signer cert, signer key, extra request options
  openssl ocsp $5 -issuer identity_ca.pem -cert "$1_cert.pem" -no_nonce -reqout "$t/req.der"
  openssl ocsp -index "$t/index.txt" -CA identity_ca.pem -rsigner "$3" -rkey "$4" -rmd sha256 \
    -reqin "$t/req.der" -respout "$2" -ndays $days
}
respond p1 ocsp_p1_good.der identity_ca.pem ca_key.pem
respond p1 ocsp_p1_good_sha256.der identity_ca.pem ca_key.pem -sha256
respond p2 ocsp_p2_good_delegated.der ocsp_signer_cert.pem ocsp_signer_key.pem
respond p3 ocsp_p3_revoked.der identity_ca.pem ca_key.pem
respond p1 ocsp_p1_wrong_signer.der permissions_ca.pem perm_ca_key.pem
# participant3 before its revocation.
sed -i 's/^R\t\(.*\)\t260101000000Z\t/V\t\1\t\t/' "$t/index.txt"
respond p3 ocsp_p3_good.der identity_ca.pem ca_key.pem
