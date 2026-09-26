#!/usr/bin/env python3
"""Prepare a private, project-local CA and deploy the authorized LAN test endpoint."""
from pathlib import Path
import os
import json
import secrets
import subprocess

ROOT = Path(__file__).resolve().parent.parent
private = ROOT / 'deploy/secrets'
private.mkdir(mode=0o700, parents=True, exist_ok=True)
os.umask(0o077)
admin = private / 'admin-token'
if not admin.exists():
    with admin.open('x') as f:
        f.write(secrets.token_hex(32) + '\n')
admin.chmod(0o444)
ca_key, ca_cert = private / 'lan-ca.key', private / 'lan-ca.crt'
if not ca_key.exists() and not ca_cert.exists():
    subprocess.run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-sha256','-days','365',
                    '-subj','/CN=aTerminal Development CA','-keyout',str(ca_key),'-out',str(ca_cert),
                    '-addext','basicConstraints=critical,CA:TRUE','-addext','keyUsage=critical,keyCertSign,cRLSign'],check=True,capture_output=True)
if not ca_key.exists() or not ca_cert.exists():
    raise SystemExit('Incomplete existing CA; refusing to silently rotate trust')
server_key, server_cert = private / 'lan-server.key', private / 'lan-server.crt'
if not server_key.exists() and not server_cert.exists():
    csr = private / 'lan-server.csr'
    subprocess.run(['openssl','req','-new','-newkey','rsa:2048','-nodes','-config',str(ROOT/'deploy/lan-cert.cnf'),
                    '-keyout',str(server_key),'-out',str(csr)],check=True,capture_output=True)
    subprocess.run(['openssl','x509','-req','-in',str(csr),'-CA',str(ca_cert),'-CAkey',str(ca_key),
                    '-CAcreateserial','-out',str(server_cert),'-days','90','-sha256',
                    '-extfile',str(ROOT/'deploy/lan-cert.cnf'),'-extensions','server'],check=True,capture_output=True)
if not server_key.exists() or not server_cert.exists():
    raise SystemExit('Incomplete existing server certificate; refusing to overwrite it')
for path in (ca_cert, server_key, server_cert):
    path.chmod(0o444)
ca_key.chmod(0o600)
subprocess.run(['openssl','verify','-CAfile',str(ca_cert),str(server_cert)],check=True)
compose = ['docker', 'compose', '-p', 'ai-terminal-dev', '-f', 'deploy/compose.lan.yaml',
           '-f', 'deploy/compose.test-network.yaml']
# Compose can reuse a container after a checkout rename even though a file-backed
# secret still binds the old absolute path. Compare strings: macOS may resolve
# both case variants, while Docker Desktop's Linux host mount cannot.
containers = subprocess.check_output(compose + ['ps', '--all', '--quiet', 'server'], cwd=ROOT, text=True).split()
recreate = False
if containers:
    inspected = json.loads(subprocess.check_output(['docker', 'inspect', *containers], text=True))
    recreate = any(not any(mount.get('Type') == 'bind'
                           and mount.get('Destination') == '/run/secrets/admin_token'
                           and mount.get('Source') == str(admin)
                           for mount in container['Mounts']) for container in inspected)
if recreate:
    print('Server secret mount uses an old checkout path; recreating LAN containers with the existing data volume.', flush=True)
# Recreate TLS as well so nginx resolves the recreated Server's current address.
subprocess.run(compose + ['up', '-d', '--no-build', '--wait'] + (['--force-recreate'] if recreate else []),
               cwd=ROOT, check=True)
subprocess.run(['curl','--fail','--silent','--show-error','--noproxy','*','--cacert',str(ca_cert),
                'https://192.168.0.36:7200/healthz'],check=True)
print('\nLAN relay: https://192.168.0.36:7200; local health: http://127.0.0.1:7201/healthz')
