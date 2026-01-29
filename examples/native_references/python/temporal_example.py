# SPDX-License-Identifier: LGPL-2.1-or-later
# Reference: Temporal Python workflow
#
# This is a REFERENCE ONLY showing how Temporal workflows look.
# See migrating_temporal example for PlexSpaces equivalent.

# Temporal workflow (for reference)
# from temporalio import workflow
# from temporalio.workflow import defn, run
#
# @defn
# class OrderWorkflow:
#     @run
#     async def run(self, order_id: str) -> str:
#         # Temporal: Execute activity
#         result = await workflow.execute_activity(
#             validate_order,
#             order_id,
#             start_to_close_timeout=timedelta(seconds=10)
#         )
#         return result
#
# PlexSpaces equivalent:
# - Actor with DurabilityFacet
# - ctx.ask(validator_actor, ValidateRequest { order_id }) for activity
# - See: examples/rust_embedded/src/bin/migrating_temporal.rs
