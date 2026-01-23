import { useMemo } from "react";
import { useQuery, useEventTypes } from "../hooks";
import { Chart } from "../components/Chart";
import { Link, useSearchParams } from "react-router";
import * as Plot from "@observablehq/plot";

export function Dashboard() {
  const [searchParams, setSearchParams] = useSearchParams();
  const eventTypes = useEventTypes();

  // Parse event type filter from URL
  const searchParamString = searchParams.toString();
  const rawEventsParam = useMemo(
    () => searchParams.get("events"),
    [searchParamString],
  );

  // Get all available event types from stats
  const allStats = useQuery({ kind: "stats" });
  const allStatsData = allStats ? allStats.toArray().map((r) => r.toJSON()) : [];

  const availableEventTypes = useMemo(() => {
    return allStatsData
      .map((s) => ({
        id: String(s.event_type),
        count: Number(s.count),
        name: eventTypes.get(String(s.event_type))?.name || `Event ${s.event_type}`,
      }))
      .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
  }, [allStatsData, eventTypes]);

  const availableEventIds = useMemo(
    () => new Set(availableEventTypes.map((e) => e.id)),
    [availableEventTypes],
  );

  const selectedEvents = useMemo(() => {
    const parsed = rawEventsParam
      ? rawEventsParam
          .split(",")
          .map((v) => v.trim())
          .filter(Boolean)
      : [];
    const filtered = parsed.filter((id) => availableEventIds.has(id));
    if (filtered.length > 0) return filtered;
    return availableEventTypes.map((e) => e.id);
  }, [rawEventsParam, availableEventIds, availableEventTypes]);

  const orderedSelectedEvents = useMemo(() => {
    const order = new Map<string, number>();
    availableEventTypes.forEach((e, idx) => order.set(e.id, idx));
    return [...new Set(selectedEvents)]
      .filter((id) => availableEventIds.has(id))
      .sort((a, b) => (order.get(a) ?? 0) - (order.get(b) ?? 0));
  }, [availableEventIds, availableEventTypes, selectedEvents]);

  // Query data with filtering
  const eventIdsFilter = orderedSelectedEvents.length === availableEventTypes.length
    ? undefined
    : orderedSelectedEvents;

  const stats = useQuery({
    kind: "stats",
    event_ids: eventIdsFilter,
  });
  const flows = useQuery({
    kind: "flows",
    event_ids: eventIdsFilter,
  });

  const statsData = stats ? stats.toArray().map((r) => r.toJSON()) : [];
  const flowsData = flows ? flows.toArray().map((r) => r.toJSON()) : [];

  const toggleEvent = (id: string) => {
    const set = new Set(orderedSelectedEvents);
    if (set.has(id)) {
      set.delete(id);
    } else {
      set.add(id);
    }
    const next = Array.from(set).filter((eid) => availableEventIds.has(eid));
    const ordered = availableEventTypes
      .filter((et) => next.includes(et.id))
      .map((et) => et.id);
    
    const params = new URLSearchParams(searchParams);
    if (ordered.length === 0 || ordered.length === availableEventTypes.length) {
      params.delete("events");
    } else {
      params.set("events", ordered.join(","));
    }
    setSearchParams(params);
  };

  const selectAll = () => {
    const params = new URLSearchParams(searchParams);
    params.delete("events");
    setSearchParams(params);
  };

  const clearAll = () => {
    const params = new URLSearchParams(searchParams);
    params.set("events", "");
    setSearchParams(params);
  };

  return (
    <div className="space-y-6">
      {/* Event Type Filter */}
      <div className="bg-white shadow rounded-lg p-6">
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium text-gray-900">Filter by event types</p>
              <p className="text-xs text-gray-500 mt-1">
                Select event types to filter flows and statistics
              </p>
            </div>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={clearAll}
                className="text-sm text-gray-600 hover:text-gray-800"
              >
                Clear all
              </button>
              <button
                type="button"
                onClick={selectAll}
                className="text-sm text-indigo-600 hover:text-indigo-800"
              >
                Select all
              </button>
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            {availableEventTypes.map((evt) => {
              const active = orderedSelectedEvents.includes(evt.id);
              return (
                <button
                  key={evt.id}
                  type="button"
                  onClick={() => toggleEvent(evt.id)}
                  className={`inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-sm transition ${active ? "bg-indigo-50 border-indigo-500 text-indigo-700" : "bg-white border-gray-200 text-gray-700 hover:bg-gray-50"}`}
                >
                  <span className="font-medium">{evt.name}</span>
                  <span className="text-xs text-gray-500">{evt.count}</span>
                </button>
              );
            })}
            {availableEventTypes.length === 0 && (
              <span className="text-sm text-gray-500">
                No events available yet.
              </span>
            )}
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-6 sm:grid-cols-2 lg:grid-cols-3">
        {/* Stat Cards */}
        <div className="bg-white overflow-hidden shadow rounded-lg">
          <div className="px-4 py-5 sm:p-6">
            <dt className="text-sm font-medium text-gray-500 truncate">
              Total Flows
            </dt>
            <dd className="mt-1 text-3xl font-semibold text-gray-900">
              {flowsData.length}
            </dd>
          </div>
        </div>
        <div className="bg-white overflow-hidden shadow rounded-lg">
          <div className="px-4 py-5 sm:p-6">
            <dt className="text-sm font-medium text-gray-500 truncate">
              Total Events
            </dt>
            <dd className="mt-1 text-3xl font-semibold text-gray-900">
              {statsData.reduce(
                (acc, curr) => acc + (Number(curr.count) || 0),
                0,
              )}
            </dd>
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Event Distribution Chart */}
        <div className="bg-white shadow rounded-lg p-6">
          <h3 className="text-lg leading-6 font-medium text-gray-900 mb-4">
            Event Distribution
          </h3>
          {statsData.length > 0 ? (
            <Chart
              options={{
                marginLeft: 150,
                y: { label: "Event Type" },
                x: { label: "Count" },
                marks: [
                  Plot.barX(statsData, {
                    y: (d: any) =>
                      eventTypes.get(String(d.event_type))?.name ||
                      d.event_type,
                    x: "count",
                    sort: { y: "x", reverse: true },
                    fill: "#4f46e5",
                  }),
                  Plot.text(statsData, {
                    y: (d: any) =>
                      eventTypes.get(String(d.event_type))?.name ||
                      d.event_type,
                    x: "count",
                    text: (d: any) => d.count,
                    dx: 4,
                    textAnchor: "start",
                  }),
                ],
              }}
            />
          ) : (
            <div className="h-64 flex items-center justify-center text-gray-500">
              No data available
            </div>
          )}
        </div>

        {/* Recent Flows List */}
        <div className="bg-white shadow rounded-lg overflow-hidden">
          <div className="px-4 py-5 sm:px-6 border-b border-gray-200">
            <h3 className="text-lg leading-6 font-medium text-gray-900">
              Recent Flows
            </h3>
          </div>
          <ul className="divide-y divide-gray-200 max-h-[500px] overflow-auto">
            {flowsData.slice(0, 20).map((flow: any) => (
              <li key={`${flow.batch_id}-${flow.flow_id}`}>
                <Link
                  to={`/flow/${flow.batch_id}/${flow.flow_id}`}
                  className="block hover:bg-gray-50"
                >
                  <div className="px-4 py-4 sm:px-6">
                    <div className="flex items-center justify-between">
                      <p className="text-sm font-medium text-indigo-600 truncate">
                        {flow.flow_id}
                      </p>
                      <div className="ml-2 flex-shrink-0 flex">
                        <p className="px-2 inline-flex text-xs leading-5 font-semibold rounded-full bg-green-100 text-green-800">
                          {Number(flow.event_count)} events
                        </p>
                      </div>
                    </div>
                    <div className="mt-2 sm:flex sm:justify-between">
                      <div className="sm:flex">
                        <p className="flex items-center text-sm text-gray-500">
                          Batch:{" "}
                          {new Date(
                            Number(flow.batch_id) / 1_000_000,
                          ).toLocaleString()}
                        </p>
                      </div>
                      <div className="mt-2 flex items-center text-sm text-gray-500 sm:mt-0">
                        <p>
                          Duration:{" "}
                          {(
                            (Number(flow.end_ts) - Number(flow.start_ts)) /
                            1_000_000
                          ).toFixed(2)}{" "}
                          ms
                        </p>
                      </div>
                    </div>
                  </div>
                </Link>
              </li>
            ))}
            {flowsData.length === 0 && (
              <li className="px-4 py-4 text-center text-gray-500">
                No flows found
              </li>
            )}
          </ul>
        </div>
      </div>
    </div>
  );
}
