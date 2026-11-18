# Multiple applications, one database

```
Server 5 (database)      tunic --router.enabled false --disable user --disable drive --disable calendar \
                              --disable food --disable intelligence --disable messages \
                              --database.enabled --database.bind 0.0.0.0 --database.token DBTOKEN \
                              --p2p.port 7000 --p2p.token MESH

Server 1 (reverse proxy + admin app)
                         tunic --database.mode remote --database.connect server5:54000 --database.token DBTOKEN \
                              --router.protocols.http 80 --router.access.admin 10.1.0.0/16 \
                              --router.reverse-proxy.static.example.com server6:8080 \
                              --join server5:7000 --p2p.token MESH

Server 2, 3, 4 (public frontend / apis)
                         tunic --database.mode remote --database.connect server5:54000 --database.token DBTOKEN \
                              --router.tags.all false --router.tags.public true --disable hpc.scheduler \
                              --join server5:7000 --p2p.token MESH
```

All servers share accounts and data through server 5. Because the data lives in one place, a login on
server 2 is valid on servers 3 and 4. Server 7 joins by passing `--join` with the address of any server.
