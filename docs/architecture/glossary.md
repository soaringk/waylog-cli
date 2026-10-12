# WayLog conversations

WayLog preserves recorded coding-agent conversations as local exports.

## Language

**Provider**:
A product’s native conversation source, with its own identity and history scope.
_Avoid_: agent executable

**Surface**:
A product variant with its own history, such as Antigravity CLI, IDE or 2.0.
_Avoid_: version when the histories coexist

**History artifact**:
A native history file or database that contains one or more conversations.
_Avoid_: session file when it contains several sessions

**Session**:
One independently identified conversation recorded by a provider.
_Avoid_: artifact

**Generation**:
A replacement recording of the same session after native migration or compaction.
_Avoid_: additional session

**Export**:
WayLog’s local representation of one provider session.
_Avoid_: native history
