%% counter.erl
%% Erlang/OTP GenServer example
-module(counter).
-behaviour(gen_server).

-export([start_link/0, increment/1, decrement/1, get/1]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2,
         terminate/2, code_change/3]).

-record(state, {count = 0}).

start_link() ->
    gen_server:start_link({local, ?MODULE}, ?MODULE, [], []).

increment(Pid) ->
    gen_server:call(Pid, increment).

decrement(Pid) ->
    gen_server:call(Pid, decrement).

get(Pid) ->
    gen_server:call(Pid, get).

init([]) ->
    {ok, #state{count = 0}}.

handle_call(increment, _From, State) ->
    NewCount = State#state.count + 1,
    {reply, NewCount, State#state{count = NewCount}};
handle_call(decrement, _From, State) ->
    NewCount = State#state.count - 1,
    {reply, NewCount, State#state{count = NewCount}};
handle_call(get, _From, State) ->
    {reply, State#state.count, State}.

handle_cast(_Msg, State) ->
    {noreply, State}.

handle_info(_Info, State) ->
    {noreply, State}.

terminate(_Reason, _State) ->
    ok.

code_change(_OldVsn, State, _Extra) ->
    {ok, State}.
