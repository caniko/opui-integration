# Authored lifecycle callbacks

Authored executable lifecycle callbacks are not part of OPUI v1 or the
`0.1.0-rc.1` package contract. RC1 applications own startup, mount, reconcile,
unmount, and application actions in Bevy code.

The frozen OPUI v1 `extensions` maps remain opaque data. Producers and
consumers must not use them as an implicit callback protocol.

A future release may add callbacks only through a separately reviewed,
versioned contract that defines its execution model, capability boundary,
ordering, failure behavior, and compatibility rules. That proposal is outside
the RC1 scope and does not block RC1.
