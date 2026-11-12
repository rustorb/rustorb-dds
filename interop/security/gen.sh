#!/bin/sh
# Makes what the DDS Security cases of interop/run.sh use, in DIR:
#
#   interop/security/gen.sh DIR [rtps_protection]
#
# - an identity CA (EC P-256) and a certificate for each side: rustorb
#   and peer, both EC P-256;
# - a permissions CA, and the governance and the permissions documents
#   signed by it (S/MIME): every topic of domains 0-230 with discovery,
#   liveliness, metadata and data encrypted, the RTPS messages protected
#   as rtps_protection says (NONE, SIGN or ENCRYPT; default NONE), and
#   both subjects allowed to publish and subscribe everything;
# - the configuration of each implementation: rustorb.properties (for
#   RUSTORB_DDS_SECURITY), cyclonedds.xml (CYCLONEDDS_URI) and fastdds.xml
#   (FASTRTPS_DEFAULT_PROFILES_FILE).
set -e
dir=$1
rtps=${2:-NONE}
mkdir -p "$dir"
cd "$dir"
days=3650
q() { openssl "$@" >/dev/null 2>&1; }
q ecparam -name prime256v1 -genkey -noout -out identity_ca_key.pem
q req -x509 -new -key identity_ca_key.pem -sha256 -days $days \
  -subj "/C=JP/O=rustorb/CN=interop identity CA" -out identity_ca.pem
q ecparam -name prime256v1 -genkey -noout -out rustorb_ec.pem
q pkcs8 -topk8 -nocrypt -in rustorb_ec.pem -out rustorb_key.pem
q req -new -key rustorb_key.pem -subj "/C=JP/O=rustorb/CN=rustorb" -out rustorb.csr
q x509 -req -in rustorb.csr -CA identity_ca.pem -CAkey identity_ca_key.pem \
  -CAcreateserial -sha256 -days $days -out rustorb_cert.pem
q ecparam -name prime256v1 -genkey -noout -out peer_ec.pem
q pkcs8 -topk8 -nocrypt -in peer_ec.pem -out peer_key.pem
q req -new -key peer_key.pem -subj "/C=JP/O=rustorb/CN=peer" -out peer.csr
q x509 -req -in peer.csr -CA identity_ca.pem -CAkey identity_ca_key.pem \
  -CAcreateserial -sha256 -days $days -out peer_cert.pem
q ecparam -name prime256v1 -genkey -noout -out permissions_ca_key.pem
q req -x509 -new -key permissions_ca_key.pem -sha256 -days $days \
  -subj "/C=JP/O=rustorb/CN=interop permissions CA" -out permissions_ca.pem
rm -f ./*.csr ./*.srl rustorb_ec.pem peer_ec.pem

cat > governance.xml <<XML
<?xml version="1.0" encoding="UTF-8"?>
<dds xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
     xsi:noNamespaceSchemaLocation="http://www.omg.org/spec/DDS-SECURITY/20170901/omg_shared_ca_governance.xsd">
  <domain_access_rules>
    <domain_rule>
      <domains><id_range><min>0</min><max>230</max></id_range></domains>
      <allow_unauthenticated_participants>false</allow_unauthenticated_participants>
      <enable_join_access_control>true</enable_join_access_control>
      <discovery_protection_kind>ENCRYPT</discovery_protection_kind>
      <liveliness_protection_kind>ENCRYPT</liveliness_protection_kind>
      <rtps_protection_kind>$rtps</rtps_protection_kind>
      <topic_access_rules>
        <topic_rule>
          <topic_expression>*</topic_expression>
          <enable_discovery_protection>true</enable_discovery_protection>
          <enable_liveliness_protection>true</enable_liveliness_protection>
          <enable_read_access_control>true</enable_read_access_control>
          <enable_write_access_control>true</enable_write_access_control>
          <metadata_protection_kind>ENCRYPT</metadata_protection_kind>
          <data_protection_kind>ENCRYPT</data_protection_kind>
        </topic_rule>
      </topic_access_rules>
    </domain_rule>
  </domain_access_rules>
</dds>
XML

grant() {
  cat <<XML
    <grant name="$1">
      <subject_name>$2</subject_name>
      <validity>
        <not_before>2020-01-01T00:00:00</not_before>
        <not_after>2100-01-01T00:00:00</not_after>
      </validity>
      <allow_rule>
        <domains><id_range><min>0</min><max>230</max></id_range></domains>
        <publish><topics><topic>*</topic></topics></publish>
        <subscribe><topics><topic>*</topic></topics></subscribe>
      </allow_rule>
      <default>DENY</default>
    </grant>
XML
}
{
  echo '<?xml version="1.0" encoding="UTF-8"?>'
  echo '<dds xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"'
  echo '     xsi:noNamespaceSchemaLocation="http://www.omg.org/spec/DDS-SECURITY/20170901/omg_shared_ca_permissions.xsd">'
  echo '  <permissions>'
  grant rustorb "C=JP,O=rustorb,CN=rustorb"
  grant peer "C=JP,O=rustorb,CN=peer"
  echo '  </permissions>'
  echo '</dds>'
} > permissions.xml
for f in governance permissions; do
  q smime -sign -in $f.xml -text -out $f.p7s -signer permissions_ca.pem -inkey permissions_ca_key.pem
done

d=$(pwd)
cat > rustorb.properties <<P
dds.sec.auth.identity_ca=file:$d/identity_ca.pem
dds.sec.auth.identity_certificate=file:$d/rustorb_cert.pem
dds.sec.auth.private_key=file:$d/rustorb_key.pem
dds.sec.access.permissions_ca=file:$d/permissions_ca.pem
dds.sec.access.governance=file:$d/governance.p7s
dds.sec.access.permissions=file:$d/permissions.p7s
P

cat > cyclonedds.xml <<XML
<?xml version="1.0" encoding="UTF-8"?>
<CycloneDDS xmlns="https://cdds.io/config">
  <Domain id="any">
    <Security>
      <Authentication>
        <IdentityCertificate>file:$d/peer_cert.pem</IdentityCertificate>
        <IdentityCA>file:$d/identity_ca.pem</IdentityCA>
        <PrivateKey>file:$d/peer_key.pem</PrivateKey>
      </Authentication>
      <AccessControl>
        <PermissionsCA>file:$d/permissions_ca.pem</PermissionsCA>
        <Governance>file:$d/governance.p7s</Governance>
        <Permissions>file:$d/permissions.p7s</Permissions>
      </AccessControl>
    </Security>
  </Domain>
</CycloneDDS>
XML

prop() { echo "            <property><name>$1</name><value>$2</value></property>"; }
{
  echo '<?xml version="1.0" encoding="UTF-8"?>'
  echo '<dds xmlns="http://www.eprosima.com/XMLSchemas/fastRTPS_Profiles">'
  echo '  <profiles>'
  echo '    <participant profile_name="secure" is_default_profile="true">'
  echo '      <rtps>'
  echo '        <propertiesPolicy>'
  echo '          <properties>'
  prop dds.sec.auth.plugin builtin.PKI-DH
  prop dds.sec.auth.builtin.PKI-DH.identity_ca "file://$d/identity_ca.pem"
  prop dds.sec.auth.builtin.PKI-DH.identity_certificate "file://$d/peer_cert.pem"
  prop dds.sec.auth.builtin.PKI-DH.private_key "file://$d/peer_key.pem"
  prop dds.sec.access.plugin builtin.Access-Permissions
  prop dds.sec.access.builtin.Access-Permissions.permissions_ca "file://$d/permissions_ca.pem"
  prop dds.sec.access.builtin.Access-Permissions.governance "file://$d/governance.p7s"
  prop dds.sec.access.builtin.Access-Permissions.permissions "file://$d/permissions.p7s"
  prop dds.sec.crypto.plugin builtin.AES-GCM-GMAC
  echo '          </properties>'
  echo '        </propertiesPolicy>'
  echo '      </rtps>'
  echo '    </participant>'
  echo '  </profiles>'
  echo '</dds>'
} > fastdds.xml
