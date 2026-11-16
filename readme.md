# Tunic
## Nested Crates
### Crate Dependency Tree
- tunic-core
- tunic-database
- tunic-element
- tunic-router
- tunic-scheduler
    Requirement:
    - tunic-database
    Optional on feature web-client or api-crud:
    - tunic-router
    - tunic-element
- tunic-user
    Requirement:
    - tunic-database
    Optional on feature web-client or api-crud:
    - tunic-router
    - tunic-element
- tunic-message
    Requirement:
    - tunic-user
    Optional on feature web-client or api-crud:
    - tunic-router
    - tunic-element
    Optional on feature message-connections:
    - tunic-xmpp
    - tunic-signal
    - tunic-simplex
- tunic-calendar
    Requirement:
    - tunic-user
    Optional on feature web-client or api-crud:
    - tunic-router
    - tunic-element
- tunic-food
    Requirement:
    - tunic-user
    Optional on feature web-client or api-crud:
    - tunic-router
    - tunic-element
- tunic-filesystem
    Requirement:
    - tunic-user
    Optional on feature web-client or api-crud:
    - tunic-router
    - tunic-element
- tunic-macro

### Crate Features
* Application and cache may be combined
application: 
    Runs application which runs in-memory or connects to database
cache: 
    Runs application-database cacheing which stores short-term data
database:
    Runs database which stores long-term data on filesystem.
    A network port is opened to support database connections. Serialized data is transmitted over this interface.
    Supports storing data in memory, on the filesystem, or on a separate database server.
    TODO: Connect to sql-based server and handle automatic sql querys
web-client: 
    Adds a router which serves http webpages
    Requires: application
api-crud: 
    Adds a router which serves http apis
    Requires: application
message-connections: 
    Reads config options for handling extra message protocols
p2p-connections: 
    Reads config options for handling p2p protocols

### Crate Config
If feature is added, a configuration must be provided. Configuration is found through environment variables, tunic.toml, and command-line arguments. When the same option is specified in multiple sources, the higher-priority source takes precedence: command-line arguments over tunic.toml, and tunic.toml over environment variables.

## Proc Macros
TODO: Create index field (field is list of ids, which map to other table) using a proc macro
TODO: Create joint table (field is list of ids, which map to table which is created by other proc macro) using a proc macro


## Todo
WMLC (Without Modifing the Library Code)
- Convert struct definition in one programming language to other language
- Convert userlist (for PAM, LDAP, SQL, etc), to serialized version in database
- WMLC, add fields to user struct
- WMLC, add scopes to main Scope
- FIX, config can change to a different http or database location (custom name: public, secret, admin, etc). Right now all routers are nested onto a root router only
- FIX, move clrud handlers to table and list implementations. Those implementations call functions which are implemented on structs
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
