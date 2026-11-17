//!
//! User
//!	    Implementation: Handle user, session
//!	    mod feature: Scope, router_all, router_crud
//!	    Crud:
//!	      - Self Config[Get, Set, List]
//!	    	- User[Create, Get, List, Update, Delete]
//!	    	    {timestamps:u64, id:Uuid, permission_index:Vec<Uuid>, playlist_index:Vec<Uuid>, chatgroup_index:Vec<Uuid>, session_index:Vec<Uuid>}
//!      		- PermissionIndex[Create, List, Delete]
//!          - PlaylistIndex[Create, List, Delete]
//!      		- ChatgroupIndex[Create, List, Delete]
//!	    	- Session[Get, List, Delete] #Not wrapped by user struct because direct lookup needs to happen
//!	    	    {timestamps:u64, id:Uuid, user_id:Uuid}
//!	    Dashboard: Would also be called admin dashboard. Crud is accessible from here.
//!
