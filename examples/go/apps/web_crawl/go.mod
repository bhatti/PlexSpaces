module github.com/bhatti/PlexSpaces/examples/go/apps/web_crawl

go 1.25.0

require github.com/bhatti/PlexSpaces/sdks/go v0.1.3

// TODO: Remove replace directive after publishing SDK v0.1.4 with stateEnvelope/routerStateEnvelope fixes.
// Published usage: require github.com/bhatti/PlexSpaces/sdks/go v0.1.4
replace github.com/bhatti/PlexSpaces/sdks/go => ../../../../sdks/go
