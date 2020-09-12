# Earth 2150 EarthNet message protocol (after login)

After the login process, the message protocol is purely text-based. Messages are terminated by a `'\0'`.
Each message begins with a command, typically of the form `/command`, although some server messages also begin with
a `$` or `&` instead of `/`. Then follows a list of parameters to the command. Parameters are separated by space,
multi-word parameters need to be put into quotation marks. The final argument to a command will consume any left-over
words, if any.

NOTE: if the client types a message beginning with `/` into the chat, it will be sent verbatim, so the list of
potential commands from the client is theoretically limitless and needs to be handled, either by dropping unknown
commands or by explicitly converting them into a chat message.

## Client messages

- `/send "<chat message>"`:
  Sends a chat message to all users in the same channel
