#!/usr/bin/env python3
"""A slow TCP relay, for recording the user guide's clips (see record-help-clips.sh).

Forwards a loopback port to another, at most RATE bytes a second each way per
connection, so that copying a few megabytes takes long enough to watch.

Usage: clip-throttle.py LISTEN_PORT TARGET_PORT RATE
"""

import asyncio
import sys

CHUNK = 16 * 1024


async def pump(reader, writer, rate):
    """Copy one direction of a connection, pausing after each chunk to keep to rate."""
    try:
        while data := await reader.read(CHUNK):
            writer.write(data)
            await writer.drain()
            await asyncio.sleep(len(data) / rate)
    except OSError:
        pass
    finally:
        writer.close()


async def main():
    listen, target, rate = int(sys.argv[1]), int(sys.argv[2]), float(sys.argv[3])

    async def relay(client_reader, client_writer):
        try:
            server_reader, server_writer = await asyncio.open_connection("127.0.0.1", target)
        except OSError:
            client_writer.close()
            return
        await asyncio.gather(
            pump(client_reader, server_writer, rate),
            pump(server_reader, client_writer, rate),
        )

    # Both loopback addresses: "files.localhost" resolves to either.
    server = await asyncio.start_server(relay, ["127.0.0.1", "::1"], listen)
    async with server:
        await server.serve_forever()


asyncio.run(main())
