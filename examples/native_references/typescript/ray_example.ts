// SPDX-License-Identifier: LGPL-2.1-or-later
// Reference: Ray distributed computing pattern
//
// This is a REFERENCE ONLY showing how Ray tasks look.
// See migrating_ray example for PlexSpaces equivalent.

// Ray (Python, for reference):
// @ray.remote
// def compute(x):
//     return x * 2
//
// ref = compute.remote(42)
// result = ray.get(ref)
//
// PlexSpaces equivalent:
// - Actor definition with handle_message
// - ctx.ask(compute_actor, ComputeRequest(42)) for remote call
// - See: examples/rust_embedded/src/bin/migrating_ray.rs
