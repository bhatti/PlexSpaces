// SPDX-License-Identifier: LGPL-2.1-or-later
// Reference: Orleans virtual actor pattern
//
// This is a REFERENCE ONLY showing how Orleans grains look.
// See migrating_orleans example for PlexSpaces equivalent.

// Orleans (C#, for reference):
// public interface IPlayerGrain : IGrainWithGuidKey {
//     Task<int> GetScore();
//     Task SetScore(int score);
// }
//
// var player = GrainFactory.GetGrain<IPlayerGrain>(playerId);
// int score = await player.GetScore();
//
// PlexSpaces equivalent:
// - Actor with VirtualActorFacet (auto-activation)
// - node.get_or_activate(actor_id) for grain reference
// - See: examples/rust_embedded/src/bin/migrating_orleans.rs

package main
