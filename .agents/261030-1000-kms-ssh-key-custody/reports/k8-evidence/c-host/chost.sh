P=$(cat fx/port); PB=$(cat fx/port_b)
O="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o IdentitiesOnly=yes -o PasswordAuthentication=no -o KbdInteractiveAuthentication=no -o BatchMode=yes -o KexAlgorithms=curve25519-sha256 -o Ciphers=chacha20-poly1305@openssh.com -o HostKeyAlgorithms=ssh-ed25519 -p"
echo "=== c1 authorized key A, main port: accepted"; ssh $O $P -i keys/A tester@127.0.0.1 'echo c1-ok'; echo rc=$?
sleep 1
echo "=== c2 unauthorized key B, main port: refused"; ssh $O $P -i keys/B tester@127.0.0.1 'echo c2-ok'; echo rc=$?
sleep 1
echo "=== c3 probe without signature (-i A.pub, no agent)"; SSH_AUTH_SOCK= ssh $O $P -i keys/A.pub tester@127.0.0.1 'echo c3-ok'; echo rc=$?
sleep 1
echo "=== c4 key B on port B: accepted"; ssh $O $PB -i keys/B tester@127.0.0.1 'echo c4-ok'; echo rc=$?
sleep 1
echo "=== c5 key A on port B: refused"; ssh $O $PB -i keys/A tester@127.0.0.1 'echo c5-ok'; echo rc=$?
sleep 1
echo "=== c6 TCP connect then close, no authentication"; timeout 5 bash -c "exec 3<>/dev/tcp/127.0.0.1/$P; sleep 1" ; echo rc=$?
sleep 1
echo "=== c7 probe of unauthorized pub on port B (-i A.pub)"; SSH_AUTH_SOCK= ssh $O $PB -i keys/A.pub tester@127.0.0.1 'echo c7-ok'; echo rc=$?
sleep 1
echo "=== c8 fingerprints"; ssh-keygen -lf keys/A.pub; ssh-keygen -lf keys/B.pub
