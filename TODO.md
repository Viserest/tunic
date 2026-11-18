
# TODO

## Features

Database(Section)
User
Calendar
Drive
Food
Intelligence
Messages(Section)
P2P(Section)

## Interface Examples

User
  Tables:  User, Session
  Pages:   Dashboard (/user/dashboard), Settings (/user/settings), etc
  APIs:    LCRUD /api/v1/{user, sessions}
Drive
  Tables:  Directory, File, Revisions
  Pages:   Files+Directory (/file)
  APIs:    LCRUD /api/v1/{directory, file, revision}

## Table

### Table Examples

```rust
#[repr(u8)]
enum UserRole {
    Subscriber,
    User,
    Both, // Subscriber and User
    Admin,
    Root,
}

#[tunic_table]
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
    #[tunic_event(on_cu(|init| crypto::hash_password(init)))]
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

fn unix_now() -> u64 {
    ...
}

mod crypto {
    fn hash_password(password: String) -> String {
        ...
    }
}
```

### Table Field Attributes

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
