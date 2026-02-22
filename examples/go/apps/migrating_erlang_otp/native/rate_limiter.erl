%% rate_limiter.erl
%% Erlang/OTP GenServer - Sliding Window Rate Limiter
%%
%% Native Erlang/OTP implementation of the same rate limiter for comparison.
%% Uses gen_server with ETS tables for high-performance sliding window tracking.
%%
%% Usage:
%%   {ok, Pid} = rate_limiter:start_link(#{window_ms => 60000, max_requests => 100}).
%%   {ok, allowed}  = rate_limiter:check_rate(Pid, <<"client-1">>).
%%   {error, denied} = rate_limiter:check_rate(Pid, <<"client-1">>).  % after limit
%%   Stats = rate_limiter:stats(Pid).

-module(rate_limiter).
-behaviour(gen_server).

-export([start_link/1, check_rate/2, get_client_status/2, stats/1, reset_client/2]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2,
         terminate/2, code_change/3]).

-record(state, {
    window_ms = 60000 :: non_neg_integer(),
    max_requests = 100 :: non_neg_integer(),
    clients :: ets:tid(),
    total_checks = 0 :: non_neg_integer(),
    total_allowed = 0 :: non_neg_integer(),
    total_denied = 0 :: non_neg_integer()
}).

-record(client_window, {
    client_id :: binary(),
    timestamps = [] :: [non_neg_integer()],
    allowed = 0 :: non_neg_integer(),
    denied = 0 :: non_neg_integer()
}).

%% ========================================================================
%% Public API
%% ========================================================================

start_link(Config) ->
    gen_server:start_link({local, ?MODULE}, ?MODULE, Config, []).

check_rate(Pid, ClientID) ->
    gen_server:call(Pid, {check_rate, ClientID}).

get_client_status(Pid, ClientID) ->
    gen_server:call(Pid, {get_client_status, ClientID}).

stats(Pid) ->
    gen_server:call(Pid, stats).

reset_client(Pid, ClientID) ->
    gen_server:call(Pid, {reset_client, ClientID}).

%% ========================================================================
%% GenServer Callbacks
%% ========================================================================

init(Config) ->
    WindowMs = maps:get(window_ms, Config, 60000),
    MaxRequests = maps:get(max_requests, Config, 100),
    Table = ets:new(rate_limiter_clients, [set, private, {keypos, #client_window.client_id}]),
    {ok, #state{
        window_ms = WindowMs,
        max_requests = MaxRequests,
        clients = Table
    }}.

handle_call({check_rate, ClientID}, _From, State) ->
    #state{window_ms = WindowMs, max_requests = MaxReqs, clients = Table} = State,

    Now = erlang:system_time(millisecond),
    Cutoff = Now - WindowMs,

    %% Get or create client window
    Window = case ets:lookup(Table, ClientID) of
        [W] -> W;
        [] -> #client_window{client_id = ClientID}
    end,

    %% Slide window: remove expired timestamps
    ActiveTimestamps = [T || T <- Window#client_window.timestamps, T > Cutoff],

    %% Check limit
    case length(ActiveTimestamps) < MaxReqs of
        true ->
            NewWindow = Window#client_window{
                timestamps = ActiveTimestamps ++ [Now],
                allowed = Window#client_window.allowed + 1
            },
            ets:insert(Table, NewWindow),
            Remaining = MaxReqs - length(ActiveTimestamps) - 1,
            NewState = State#state{
                total_checks = State#state.total_checks + 1,
                total_allowed = State#state.total_allowed + 1
            },
            {reply, {ok, allowed, #{remaining => Remaining, limit => MaxReqs}}, NewState};
        false ->
            NewWindow = Window#client_window{
                timestamps = ActiveTimestamps,
                denied = Window#client_window.denied + 1
            },
            ets:insert(Table, NewWindow),
            RetryAfterMs = case ActiveTimestamps of
                [Oldest | _] -> Oldest + WindowMs - Now;
                [] -> 0
            end,
            NewState = State#state{
                total_checks = State#state.total_checks + 1,
                total_denied = State#state.total_denied + 1
            },
            {reply, {error, denied, #{retry_after_ms => RetryAfterMs}}, NewState}
    end;

handle_call({get_client_status, ClientID}, _From, State) ->
    #state{window_ms = WindowMs, max_requests = MaxReqs, clients = Table} = State,
    Now = erlang:system_time(millisecond),
    Cutoff = Now - WindowMs,
    case ets:lookup(Table, ClientID) of
        [Window] ->
            Active = [T || T <- Window#client_window.timestamps, T > Cutoff],
            Remaining = max(0, MaxReqs - length(Active)),
            {reply, {ok, #{
                client_id => ClientID,
                current_count => length(Active),
                remaining => Remaining,
                limit => MaxReqs,
                total_allowed => Window#client_window.allowed,
                total_denied => Window#client_window.denied
            }}, State};
        [] ->
            {reply, {ok, #{
                client_id => ClientID,
                current_count => 0,
                remaining => MaxReqs,
                limit => MaxReqs
            }}, State}
    end;

handle_call(stats, _From, State) ->
    #state{total_checks = Checks, total_allowed = Allowed,
           total_denied = Denied, clients = Table,
           window_ms = WindowMs, max_requests = MaxReqs} = State,
    NumClients = ets:info(Table, size),
    DenyRate = case Checks of
        0 -> 0.0;
        _ -> Denied / Checks * 100
    end,
    {reply, {ok, #{
        config => #{window_ms => WindowMs, max_requests => MaxReqs},
        total_checks => Checks,
        total_allowed => Allowed,
        total_denied => Denied,
        deny_rate_pct => DenyRate,
        active_clients => NumClients
    }}, State};

handle_call({reset_client, ClientID}, _From, State) ->
    ets:delete(State#state.clients, ClientID),
    {reply, ok, State}.

handle_cast(_Msg, State) ->
    {noreply, State}.

handle_info(_Info, State) ->
    {noreply, State}.

terminate(_Reason, State) ->
    ets:delete(State#state.clients),
    ok.

code_change(_OldVsn, State, _Extra) ->
    {ok, State}.
