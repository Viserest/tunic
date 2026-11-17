//!
//! Message
//! 	Implementation: Convert all message sources to base chatgroups and messages
//! 	mod feature: Config, Scope, router_all, daemon_run
//! 	Crud:
//! 	  - Self Config[Get, Set, List]
//! 		- Chatgroup[Create, Get, List, Update, Delete]   #UserTable <-> ChatgroupTable -> MessageList#
//! 				{timestamps:u64, id:Uuid, user_index:Vec<Uuid>, messages:Vec<Message>}
//!   		- UserIndex[Create, List, Delete] #Lookup users
//! 		  - Message[Create, Get, List, Update, Delete]
//! 				{timestamps:u64, id:Uuid, user_id:Uuid, thread_parent:Uuid, thread_children:Vec<Uuid>}
//! 	Dashboard: Http, Database
//! 	Xmpp: Socket
//! 	Signal: Socket
//! 	Simplex: Socket
//!
//!
