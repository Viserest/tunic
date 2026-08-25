Router
  Login: POST, GET
  Admin
    Dashboard: GET
    Statistics: POST
    Password-Reset: POST
  Crud
    User: POST, GET, LIST, DELETE
      Playlist: POST, GET, LIST, DELETE
      Message: POST, GET, LIST, DELETE
    Session: GET, LIST, DELETE
    File: POST, GET, LIST, DELETE
      Permission: POST, GET, LIST, DELETE
  Storage
    Dashboard: GET
  Media
    Dashboard: GET
    Streaming: POST
  Messages
    Dashboard: GET
    Notification: Websocket
  Peer-to-peer config
    Dashboard: GET
