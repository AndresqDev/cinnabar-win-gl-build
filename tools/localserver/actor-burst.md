# Actor burst fixture

Start the local Dragonfly server with `-actor-burst`. Through the headless client MCP,
use `chat` to send `/actorburst 128 shared`, `/actorburst 128 skins`, or
`/actorburst 128 geometry`. Each command replaces the previous crowd near the player;
`/actorburst clear` removes it. The command reports the supported count limit when an argument is out of range.

`shared` repeats identical generated skins and models, `skins` varies only the texels,
and `geometry` varies both. The fixture uses original generated images and models,
synthetic identities, and no remote accounts. Capture an idle baseline, one cold burst,
a steady crowd, then a clear and repeated burst to distinguish preparation from reuse.

For cold resource-pack publication, start with `-actor-burst-artwork 128`. The server
offers one original generated pack with distinct texture sizes through normal pack
delivery. Capture the join, then send `/actorburst 128 artwork` to show its custom
actors. Reconnect to exercise session publication again; resource packs are immutable
during a connection. An artwork command cannot exceed the count offered at startup.

`/actorburst 128 complex` adds many articulated detail bones and small cubes per player. Models and pixels vary independently across the crowd; use this case to
measure cold source parsing and mesh building, then clear the crowd normally.

`/actorburst N swing` stands N identical sword-holding players three blocks ahead, facing back,
with the standard player bone hierarchy. They swing on the client's input ticks: twice ten ticks
apart, then every other tick, repeating every 40 ticks. `/actorburst N strafe` sways them 1.5 blocks
sideways while facing the player and swinging every other tick, as PvP spam-clicking does. Use them
to capture remote attack animations.
