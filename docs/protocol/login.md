# Earth 2150 EarthNet Login protocol

Upon establishing connection, the sequence of messages goes like this:
- Client: `<ClientIdent>`
- Server: `<ServerIdent> / <ServerReject>`
- Client: `<ClientLogin>`
- Server: `<ServerWelcome> / <ServerReject>`

Login messages use a binary message protocol, in addition the messages are zlib-compressed for obfuscation
purposes. There is no further protection, passwords and other data are transmitted in the clear.

NOTE: technically, all string values in these messages are ASCII encoded (or specific language dialects, e.g. ISO8859-1),
not UTF8-encoded as Rust's strings.

Each compressed message looks like this:
```
<length: u32><compressed: [u8]>
```
- `length` is the total length of the message
- `compressed` is the block of zlib-compressed bytes of length `length - 4`

After decompression with the zlib deflate algorithm, the messages can be individually parsed.

## `<ClientIdent>`

```
<game_version_guid: [u8; 16]><lang_len: u32><lang: &str>
```
- `game_version_guid`: a Windows GUID representing the client's game version, in 16 byte binary representation
- `lang_len`: length of the language identifier
- `lang`: a string identifying the language version of the client (e.g. `ENG`, `GER`)

## `<ServerIdent>`

```
<0u32><unknown>
```

Begins with a `0` indicating a success message. Rest of the message content is unknown, the client doesn't seem
to care particularly, but some contents do crash the client.

## `<ClientLogin>`

```
<user_length: u32><user: &str><password_length: u32><password: &str><0u32><0u32>
```
- `user_length`: length of the username
- `user`: actual username
- `password_length`: length of the password
- `password`: actual password (in clear text)

In case of a login attempt to an existing account, the message ends with two zeroes. If trying to create a new account,
additional data is appended to the message (TODO: write down)

## `<ServerWelcome>`

```
<0u32><content_length: u32><content>

<content>:
<ident_len: u32><ident: &str><welcome_len: u32><welcome: &str>
<unknown: u64><unknown: u32><players_total: u32><players_online: u32><channels_total: u32>
<games_total_a: u32><games_total_b: u32><unknown: u32><games_available: u32><unknown: u32>
<game_versions: list><unknown: list><unknown: list>
<unknown: u8>
<channel_len: u32><channel: &str>
<unknown (can be set to [0u8; 40])>

<list>:
{ <id: u8><name_len: u32><name: &str> } x N
<0xffu8> // list termination marker
```
- `ident`: an identifying name for the server (does not appear to be used by the client anywhere)
- `welcome`: initial message from the server displayed in the chat lobby
- `game_versions`: a list of names for supported game versions by the server (to separate available games)
- `channel`: the initial channel when the client joins the lobby

## `<ServerReject>`

```
<2u32><reason_length: u32><reason: &str>
```
- `reason_length`: length of the given reason for the rejection
- `reason`: the reason given for the rejection by the server (e.g. password invalid). Text may be translated if it
  matches an entry in a lang file.
  