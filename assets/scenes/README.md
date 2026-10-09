# Musical worlds

`four-worlds.json` is the editable native scene source, embedded in the game build and rendered by `crates/cocobeat-runtime/src/scene/worlds.rs`

Each world owns three arrangements, five landmarks per arrangement, an atmosphere gradient and a ground palette; each landmark tuple contains its kind, world-space origin in meters, and uniform scale

Neon moves from lantern market to elevated railway to rooftop performance; Forest moves from woodland to stream to canopy; Candy moves from toy street to rides to balloon celebration; StarSea moves from crystal canyon to floating islands to star-ring theatre

Landmarks share seven native meshes and ten material slots per world; authored profile meshes form curved roofs, organic trunks and faceted crystals, with no external model, texture, font or runtime importer

Each world also owns a persistent near/middle/far backdrop: curb planters and framed city facades, forest ferns and layered canopies, striped park paths and flag stalls, or stratified island fragments and distant stars; these remain between the three authored section arrangements to keep the environment spatially connected

Broad roof, tree-canopy and mushroom-stem surfaces use physical materials; narrow roof edges, windows, lamps and mushroom spots carry emission so lighting preserves surface volume

Crystal facets and amusement-ride shells use paired physical color materials; crystal seams, small cabin windows and the carousel rim retain emission while the main surfaces respond to lights and reflections

The center lane remains reserved for characters and timing cues; ground landmarks stay outside x ±6 meters before the Stage offset, and centered overhead halos retain at least six meters of vertical clearance

Section visibility and its 1.2-second reveal derive from the presentation section cursor; train travel, rotating rides and floating details derive from SongTime, so pause holds their pose and seek reconstructs it

The file and native geometry are original CoCoBeat artwork under the repository MPL-2.0 license; visual and performance acceptance require the production renderer rather than this data validation alone
