#cargo build --package v8-isolates-comparison > err 2>&1
cargo build --release > err 2>&1
if [ $? -eq 0 ];
then
cargo run --release --bin v8_isolates_comparison > err 2>&1
else
	cat err
	echo build failure
fi;
if [ $? -ne 0 ];
then
	cat err
	echo run failure
fi;
