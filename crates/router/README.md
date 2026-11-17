# Tunic Router

List of routes, tags and a handler function

## Protocols

- Http
- Gopher
- Gemini

## Handles

Http
- TcpListener at port 80: Handles Http 0.9/1.0/1.1/2
- Pseudo-UdpListener at port 80: Handles Http 3
Gopher
- TcpListener at port 70: Handles Gopher traffic
Gemini
- TcpListener at port 1965: Handles Gemini traffic

## Concept

1. Handles convert the stream of data into crate Request's.  
2. Router takes in a crate Request, runs it into the handle function to get a crate Response.
3. Handles convert the crate Response into a stream of data.

## Core

### Errors

ErrorServe
ErrorRequest
ErrorResponse

### Request

Request
RequestBuilder

### Response

Response
ResponseBuilder

### Version

Combines http-version, gopher, and gemini

### Status

#### StatusCode

Combines http-status, and gemini-statuscode

#### StatusKind

Links between http-statuscode, and gemini-statuscode
