module github.com/bhatti/plexspaces/sdks/go

go 1.25.0

require (
	buf.build/gen/go/bufbuild/protovalidate/protocolbuffers/go v1.36.11-20260209202127-80ab13bee0bf.1
	github.com/grpc-ecosystem/grpc-gateway/v2 v2.28.0
	google.golang.org/genproto/googleapis/api v0.0.0-20260316172706-e463d84ca32d
	google.golang.org/protobuf v1.36.11
)

// Generated proto files (make proto-go) use go_package = "github.com/bhatti/plexspaces/gen/go/..."
// but live locally under ./plexspaces/proto/. This replace maps the declared import path to
// the local directory so they resolve without a separate Go module or registry.
replace github.com/bhatti/plexspaces/gen/go => ./plexspaces/proto
