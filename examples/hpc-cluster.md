# One application, an HPC cluster, multiple databases

```
Server 5 (database 1)
tunic \
  --router.enabled false \
  --database.enabled \
  --database.bind 0.0.0.0 \
  --database.token T1 \
  --disable user \
  --disable drive \
  --disable calendar \
  --disable food \
  --disable intelligence \
  --disable messages \
  --p2p.port 7000 \
  --p2p.token MESH
Server 6 (database 2)
same, with --database.token T2

Server 1 (scheduler dashboard)
tunic \
  --enable hpc.scheduler \
  --user.admin-password '...' \
  --database.mode memory \
  --database.route 'user=T1@server5:54000' \
  --database.route 'hpc=T2@server6:54000' \
  --p2p.port 7001 \
  --p2p.token MESH \
  --join server5:7000

Server 2, 3, 4 (workers; no port needed)
tunic \
  --router.enabled false \
  --enable hpc.worker \
  --hpc.worker.threads 0 \
  --disable user \
  --disable drive \
  --disable calendar \
  --disable food \
  --disable intelligence \
  --disable messages \
  --database.mode memory \
  --join server1:7001 \
  --p2p.token MESH
```

Submit work as an administrator:

```
curl \
  -c jar \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"..."}' \
  http://server1:8080/user/login

curl \
  -b jar \
  -d '{"kind":"primes","reduce":"sum","range":{"start":0,"end":100000000,"chunks":64}}' \
  http://server1:8080/hpc/batches

curl \
  -b jar \
  http://server1:8080/hpc/batches/<id>
```

Built-in job kinds: `echo`, `sleep`, `primes`, `pi`, `sha256_chain`, `wordcount`. Add your own in a library build with `App::tasks(TaskRegistry)` (see README).
