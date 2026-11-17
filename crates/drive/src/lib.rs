//!
//! Filesystem
//!	    Implementation: Directorylist, filelist, and permissionlist
//!	    mod feature: router_crud
//!	    Crud:
//!	      - Self Config[Get, Set, List]
//!	    	- Permission[Create, Get, List, Update, Delete] #UserTable(Index) <-> PermissionTable <-> Resource: DirectoryTable(Index)/FileTable(Index)#
//!	    	    {timestamps:u64, id:Uuid, user_id:Uuid, resource_id:Uuid}
//!	    	- Directory[Create, Get, List, Update, Delete]
//!	    	    {timestamps:u64, id:Uuid, parent_id:Uuid, permission_index:Vec<Uuid>, files:Vec<Uuid>}
//!      		- PermissionIndex[Create, List, Delete]
//!	    	- File[Create, Get, List, Update, Delete]
//!	    	    {timestamps:u64, id:Uuid, parent_id:Uuid, permission_index:Vec<Uuid>, data:Option<Vec<u8>>, size:usize}
//!      		- PermissionIndex[Create, List, Delete]
//! Media
//! 	Implementation: Playlists
//! 	mod feature: Config, Scope, router_all
//! 	Crud:
//! 	  - Self Config[Get, Set, List]
//! 		- Playlist[Create, Get, List, Update, Delete]    #UserTable <-> PlaylistTable -> FileTable#
//! 		    {timestamps:u64, id: Uuid, user_index:Vec<Uuid>, file_index:Vec<Uuid>}
//!       - UserIndex[Create, List, Delete] #Lookup users
//!       - FileIndex[Create, List, Delete] #Lookup files
//! 	Dashboard: Http, Database
//! Storage
//! 	Implementation: Handle user, directory, and file lists
//! 	mod feature: Config, Scope, router_all
//! 	Crud:
//! 	  - Self Config[Get, Set, List]
//! 		- filesystem:crud
//! 	Dashboard: Http, Database
//!
