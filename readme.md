
# Tunic
## Compile Time Features
All features can be used at once.

### Manual
A manual setup would make base module functions public to allow your program to set them up. 
An automatic setup (default) would inject routes into your axum router.

### Apps
Includes: http, database, storage, media, message, and p2p, along with their own config.

If feature is added, a configuration must be provided. Configuration is found through environment variables, tunic.toml, and command-line arguments. When the same option is specified in multiple sources, the higher-priority source takes precedence: command-line arguments over tunic.toml, and tunic.toml over environment variables.

### Database
This feature adds:
1. **Network connectivity:** Opens a network port to support database-to-database and application-to-database connections. Serialized data is transmitted over this interface.
2. **Flexible data storage:** Supports storing data in memory, on the filesystem, or on a separate database server.


## Standard stuff
Timestamps:
```rust
created_at: u64,
accessd_at: u64,
updated_at: u64,
deleted_at: u64,
```


## Axum router uses
setup     serve
setup     routing on path/method
extract   Path
extract   Query
extract   Json
extension State
response  Json
response  StatusCode
response  Headers
response  Vec<u8>
response  String

## Host router uses
setup     routing on host header


## Todo
WMLC (Without Modifing the Library Code)
- Convert struct definition in one programming language to other language
- Convert userlist (for PAM, LDAP, SQL, etc), to serialized version in database
- WMLC, add fields to user struct
- WMLC, add scopes to main Scope
- FIX, config can change to a different http or database location (custom name: public, secret, admin, etc). Right now all routers are nested onto a root router only
- FIX, move clrud handlers to table and list implementations. Those implementations call functions which are implemented on structs
- ADD, scope requirement for features
- ADD, create index (field being a list of ids, which map to other table) using a proc macro
- ADD, create joint table (field being a list of ids, which map to table which is created by other proc macro) using a proc macro
- ADD, LDAP server
- ADD, file transfer in any protocol (TCP, ssh, rsync, google's way, amazon's way, etc)
- ADD, communication in any sql flavor


## Implementation and features
Http
	Implementation: Initial basic router
	mod feature: Config, daemon_run
Database
	Implementation: Table, List, Packets
	mod feature: Config, daemon_run
User
	Implementation: Handle user, session
	mod feature: Scope, router_all, router_crud
	Crud:
	  - Self Config[Get, Set, List]
		- User[Create, Get, List, Update, Delete]
		    {timestamps:u64, id:Uuid, permission_index:Vec<Uuid>, playlist_index:Vec<Uuid>, chatgroup_index:Vec<Uuid>, session_index:Vec<Uuid>}
  		- PermissionIndex[Create, List, Delete]
      - PlaylistIndex[Create, List, Delete]
  		- ChatgroupIndex[Create, List, Delete]
		- Session[Get, List, Delete] #Not wrapped by user struct because direct lookup needs to happen
		    {timestamps:u64, id:Uuid, user_id:Uuid}
	Dashboard: Would also be called admin dashboard. Crud is accessible from here.
Filesystem
	Implementation: Directorylist, filelist, and permissionlist
	mod feature: router_crud
	Crud:
	  - Self Config[Get, Set, List]
		- Permission[Create, Get, List, Update, Delete] #UserTable(Index) <-> PermissionTable <-> Resource: DirectoryTable(Index)/FileTable(Index)#
		    {timestamps:u64, id:Uuid, user_id:Uuid, resource_id:Uuid}
		- Directory[Create, Get, List, Update, Delete]
		    {timestamps:u64, id:Uuid, parent_id:Uuid, permission_index:Vec<Uuid>, files:Vec<Uuid>}
  		- PermissionIndex[Create, List, Delete]
		- File[Create, Get, List, Update, Delete]
		    {timestamps:u64, id:Uuid, parent_id:Uuid, permission_index:Vec<Uuid>, data:Option<Vec<u8>>, size:usize}
  		- PermissionIndex[Create, List, Delete]
Media
	Implementation: Playlists
	mod feature: Config, Scope, router_all
	Crud:
	  - Self Config[Get, Set, List]
		- Playlist[Create, Get, List, Update, Delete]    #UserTable <-> PlaylistTable -> FileTable#
		    {timestamps:u64, id: Uuid, user_index:Vec<Uuid>, file_index:Vec<Uuid>}
      - UserIndex[Create, List, Delete] #Lookup users
      - FileIndex[Create, List, Delete] #Lookup files
	Dashboard: Http, Database
Storage
	Implementation: Handle user, directory, and file lists
	mod feature: Config, Scope, router_all
	Crud:
	  - Self Config[Get, Set, List]
		- filesystem:crud
	Dashboard: Http, Database
Message
	Implementation: Convert all message sources to base chatgroups and messages
	mod feature: Config, Scope, router_all, daemon_run
	Crud:
	  - Self Config[Get, Set, List]
		- Chatgroup[Create, Get, List, Update, Delete]   #UserTable <-> ChatgroupTable -> MessageList#
				{timestamps:u64, id:Uuid, user_index:Vec<Uuid>, messages:Vec<Message>}
  		- UserIndex[Create, List, Delete] #Lookup users
		  - Message[Create, Get, List, Update, Delete]
				{timestamps:u64, id:Uuid, user_id:Uuid, thread_parent:Uuid, thread_children:Vec<Uuid>}
	Dashboard: Http, Database
	Xmpp: Socket
	Signal: Socket
	Simplex: Socket
P2p
	Implementation: Open sockets for peer-to-peer connections
	mod feature: Config, Scope, router_all, daemon_run
	Crud:
	  - Self Config[Get, Set, List]
	Dashboard: Http
	Tor: Socket
	I2p: Socket
