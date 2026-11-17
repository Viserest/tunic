
# Tunic

## Features

Application (Router):
- User
- Drive
- Calendar
- Food
- Intelligence
- Messages (Client)
- P2P (Dashboard)

Database: 
  Runs database which stores long-term data on filesystem.
  A network port is opened to support database connections. Serialized data is transmitted over this interface.
  Supports storing data in memory, on the filesystem, or on a separate database server.
  Connections can be made using other protocols (SQL, FTP, etc)

Connections:
- Messages (Relay, Intercept)
- P2P (Relay, Router)

All features can be enabled. Any distributed schema is possible.

Examples:

Multiple applications, one database:
```
Server 1: Reverse proxy, Application 1 (Admin dashboard frontend/apis)
Server 2: Application 2 (Public frontend)
Server 3: Application 3 (Public apis)
Server 4: Application 4 (Public apis)
Server 5: Database
Server 6: Static content? // This is the only reason reverse proxy is used. 
                          // All other servers send content through the communication port.
```

One application, High-performance computing cluster, Multiple databases:
```
Server 1: Application 1 (Schedular dashboard)
Server 2: HPC 1
Server 3: HPC 2
Server 4: HPC 3
Server 5: Database 1
Server 6: Database 2
```

Note: All executeables have an optional port opened for communication between the nodes, but when 
run they have to have at least one node to initial with. To add Server 7, it has to include 
an argument to look at any Server 1-6.

### Feature config

Configuration is found through environment variables, tunic.toml, and command-line arguments. 
When the same option is specified in multiple sources, the higher-priority source takes
precedence: command-line arguments over tunic.toml, and tunic.toml over environment variables.

Example:
```toml
# Web router is enabled
[router]
enabled = true

# Web router protocols which are enabled and their ports
[router.protocols]
gopher = 70
http = 80
https = 443
gemini = 1965

# All router tags are enabled
[router.tags]
all = true

# Router tag "public" is accessible from all ip addresses from any protocol
# Router tag "admin" is accessible from "10.1.0.0 - 10.1.255.255" from any protocol
[router.access]
public = "0.0.0.0"
admin = "10.1.0.0/16"

# Reverse proxy works like caddy or nginx
[router.reverse-proxy]
"example.com" = "localhost:2455"
"illustration.com" = "localhost:2456"

# Database is enabled and accessible from port 54000. Since Web Router is also 
# enabled this running binary doesnt connect back to itself on port 54000.
[database]
enabled = true
port = 54000
```

## Functionality

### Core

#### Element

Building blocks of a page for any device (phone, desktop, web).

Elements:
- Section (html div)
- Table (html table)
- Code (html code)
- Text (html paragraph)
- Link (html anchor)

Styles:
- Width is sectioned into 12-columns with margins between.

#### Router

Routing a request to a page over any protocol.

Protocols:
- Gopher
- HTTP (v1, v2, v3)
- Gemini
- Custom binary
- TLS (initially where needed)

#### Database

Storage that handles ACID or BASE, SQL or noSQL, etc. Everything is built as Rust structs, then proc macros handle complex schemas.

Example: 
A music metadata storage, artist stores a list of albums, albums stores a list of songs.

```rust
struct Artist {
    id: Uuid,
    albums: Table<Album>,
    name: String;
}
struct Album {
    id: Uuid,
    songs: Table<Song>,
    name: String;
}
struct Song {
    id: Uuid,
    title: String,
}

let suffocate = Song::new(artist_id, album_id, "Suffocate");
suffocate.artist().name = "Crossfade"; // Generates this function
suffocate.album().name = "We All Bleed"; // Generates this function
suffocate.title = "Suffocate";
```

Rust generates:
1. Song:   Method to lookup album or artist.
2. Album:  Method to lookup songs or artist.
2. Artist: Method to lookup songs or albums.

Protocols:
1. Custom binary format
2. Any sql flavor
3. Any file transfer protocol (TCP, ssh, rsync, google's way, amazon's way, etc)

### Addon

#### User

Users as social media profiles.

Extra protocols:
1. LDAP server

#### Message

Message Client: Webpage for viewing messages
Message Relay: XMPP, etc
Message Interception: Teams, Snapchat, Whatsapp, Signal, Simplex, etc

#### Calendar

#### Intelligence

#### Food

#### Drive

## Project Tree

This project can be used as a binary executable (apps/tunic) or as a rust library.

```
apps/            .
  tunic          Executable for running.
crates/          .
  ..CORE         .
  utils          .
  element        Building blocks of a page for any device (phone, desktop, web).
  router         Routing a request to a page over any protocol.
  database       Storage built using Rust structures.
  proc_macros    .
  ..FRONTEND     .
  user           User accounts. Remake of Google Accounts, Microsoft Accounts, etc.
  drive          Access files. Remake of Google Drive, or Microsoft OneDrive.
  calendar       View calendar. Remake of Google Calendar, Apple Calendar, etc.
  food           Track workouts and nutrition.
  intelligence   AI not through an LLM.
  ..EXTERNAL     .
  messages       Message client page, relay, and interception.
  p2p            Tor node, I2P router, etc.
```
