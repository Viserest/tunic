
# TODO

## Idea

These user interfaces built-in are accessed as part of tunic. A crate which uses tunic as a libray can modify into tunic built-in stuff, but also can make their own user interfaces as apps. These apps can use router/database/hpc.

## LCRUD (crud with list)

All of these must have these options:
1. App's "admin"-role can either call by his own id or by user's id
2. User can call by his own id

## Feature modifications

### Media playlists

Remove media playlists because thats not how it should work. Media is just looking at a directory of files. Everything inside of their is either movies/music/shows/etc. This is called a Media Library (looking at directory with whats inside).

Move the media user interface into a new crate. Its interface should look like netflix. This would also allow connections using jellyfin's protocol to jellyfin clients.

### Intelligence

Add math forms: geometry, trig, algebra, limit, derive, integral, series
Add prebuilt library of formulas (like F=m\*a, V=I\*R, Product Rule, cosine as series). 
Add sub-script units to all numbers (optionally shortened or long text).
Add variables with units.
Add variable unit intelligence a_meter / b_second = c_meter|second. it finds meter per second.

### Router

Use rustls by default.

## p2p vs database vs router

There may have been some confusion between what all these [p2p, remote database connections, router protocols, i2p router, tor node, xmpp daemon, signal daemon, etc] meant. 

By default when running multiple tunic daemons they can all connect over a p2p mesh network. 
All traffic between them is sent through this p2p network:
- database traffic
- remote web routing (Server A has main router, Server B has webpage routes, Server C has api routes; Server A needs to send api request to Server C)
- reverse-proxy (traffic sent externally through ip:port)
- hpc traffic

When i first specificed p2p/messaging servers, it meant this daemon ran:
- Tor node daemon
- i2p router daemon
- xmpp daemon
- signal daemon
- etc

as tokio tasks.

## Router tags and scopes

Define:
- Router scopes will be called "route tags"
- Database scopes will be called "database scopes"
- HPC scopes will be called "hpc cluster"

* All of these are formatted as hierarchies. `a/b/c`, `a/de/ls`

### Route tags

Route tags will be WAY more open, example:
Calendar router: 
- calendar/public: Webpages without authorization
- calendar/authed: Webpages with user authorization
- calendar/apis: Apis

* Route tags dont follow a schema, they can be whatever is decided. Its only to keep routes separated so that it can act as a quick microservice, example: Server A only runs calendar api, Server B only runs calendar webpages.

* This will either need to be per route, or per router (per router will push that route tag to all nested routes).

### Database scopes

I like the scheme now. If three apps use a user table, they all need to be separated.

### Hpc cluster

Now that all tunic daemons are by default and required to be a p2p node, they must have a hpc cluster scope.

## Database

Storage types:
- Application cache in memory short term storage.
- Local database (application and database in the same daemon).
- Remote database (application -> database over p2p network).
- Either database could have it in memory or on filesystem.

Needs to allow a database scope to have its data spread out. 

### Example:

Maybe app1 has 4 databases for its users. app1/application has a key to lookup, its not in application cache, and it needs to know which database has the key's value. 

This database schema needs to allow the user to tell the database which way it would store across databases. Either:
- sync all databases,
- specific keys are allocated to specific databases (using a quick algorithm),
- hierarchy structure (where database 1 -> database 2 -> database 3 [found] or database 1 -> database 2 [found]), or 
- broadcast to all databases then wait for found.

### Application/Database Caching

Optimizations:
- Just searched (keep in cache for duration)
- Time of day (user checks messages at certain time of day so pre-fetch them)
- Location (user checks email at certain restuarant so pre-fetch them)

* Hosted server must enable these optimizations explictly.

### Table

This adds:
- Table methods optionally added (list, create, read, update, and delete) using tunic_allow
- Versioning and how to update using tunic_version
- Events using on_*. Field attribute method on_c is basically default.

* You can keep json but remove it from filesystem, and remote. It stores to filesystem using binary and sends to other databases using binary.

#### Table Examples

```rust
#[repr(u8)]
enum UserRole {
    Subscriber,
    User,
    Both, // Subscriber and User
    Admin,
    Root,
}

#[derive(Record)]
struct User {
    #[tunic_allow(r)]
    #[tunic_version(v1:c(u64), v2:u(|old: u64| Ok(Uuid::from_u128(id as u128))))]
    id: Uuid,
    
    #[tunic_allow(cru)]
    #[tunic_version(v1:c)]
    name: String,
    #[tunic_allow(none)]
    #[tunic_event(on_c(true))]
    #[tunic_version(v2:c, v4:d)]
    active: bool,
    #[tunic_allow(cru)]
    #[tunic_version(v4:c)]
    username: String,
    #[tunic_allow(cu)]
    #[tunic_event(on_cu(|init| hash_password(init)))]
    #[tunic_version(v4:c)]
    password: String,
    #[tunic_allow(cru)]
    #[tunic_version(v4:c)]
    role: UserRole,
    
    #[tunic_allow(r)]
    #[tunic_event(on_c(unix_now))]
    #[tunic_version(v3:c)]
    created_at: u64,
    #[tunic_allow(r)]
    #[tunic_event(on_u(unix_now), on_c(0))]
    #[tunic_version(v3:c)]
    updated_at: u64,
    #[tunic_allow(r)]
    #[tunic_event(on_d(unix_now), on_c(0))]
    #[tunic_version(v3:c)]
    deleted_at: u64,
}
```

#### Table Field Attributes

Field argument conversions:

```rust
#[tunic_allow(...)]   <--> #[tunic(allow(...))]
#[tunic_event(...)]   <--> #[tunic(event(...))]
#[tunic_version(...)] <--> #[tunic(version(...))]
```

Field arguemnt restrictions:

```
tunic_allow: list, create, read, update, delete
tunic_event: on_list, on_create, on_read, on_update, on_delete
tunic_version: create, update, delete
```

Method argument conversions:

```
list   <--> l. Ex: on_list   <--> onl OR on_l
create <--> c. Ex: on_create <--> onc OR on_c
read   <--> r. Ex: on_read   <--> onr OR on_r
update <--> u. Ex: on_update <--> onu OR on_u
delete <--> d. Ex: on_delete <--> ond OR on_d
```

Method argument multiples:

```rust
#[tunic_allow(create, read, update)] <--> #[tunic_allow(cru)]
#[tunic_event(on_create(...), on_update(...))] <--> #[tunic_event(on_cu(...))]
```

Method arguments:

```
tunic_version: create(OLD TYPE), update(CONVERSION FUNCTION RETURNS RESULT)
tunic_event:   on_*(VALUE OR FUNCTION RETURNS VALUE)
```

create(OLD TYPE) is required when a newer version modifies the create version
