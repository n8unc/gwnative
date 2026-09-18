# GWNative

GWNative provides a native macOS application for playing Guild Wars Reforged.
This glossary distinguishes the application from the game client and its game data.

## Language

**GWNative host**:
The macOS application hosting the game client.
_Avoid_: Game client when referring to GWNative itself.

**Game client**:
ArenaNet's program for playing Guild Wars Reforged, hosted by GWNative.
_Avoid_: GWNative host, native port of the game.

**Client generation**:
A matching set of game-client files belonging to a particular patch generation.
_Avoid_: GWNative release, game image.

**Profile**:
A separate local GWNative configuration identity, distinct from a Guild Wars account.
_Avoid_: Guild Wars account, character.

**Account**:
A local launcher entry representing an existing Guild Wars login, with its own default private profile. Creating an entry does not register a new Guild Wars account.
_Avoid_: Profile, character, ArenaNet registration.

**Launcher**:
The GWNative interface for managing accounts and starting game instances, available before and during play.
_Avoid_: Game client, game window.

**Auto-login**:
Signing into an Account using its saved credentials after its game instance starts. This does not choose a character or enter the game world.
_Avoid_: Auto-launch, automatic character selection.

**Auto-launch**:
Starting an Account's game instance when the launcher application starts. Whether that instance signs in automatically is controlled separately by Auto-login.
_Avoid_: Auto-login.

**Game image**:
The game-content data used by the game client, distinct from the client program itself.
_Avoid_: Snapshot without qualification, game client.

**Chunk**:
A piece of a game image.
_Avoid_: Game image when referring to a single piece.

**Game-state snapshot**:
A limited observation of game state at a particular moment, such as the player's location and selected target.
_Avoid_: Game image, saved game, snapshot without qualification.
