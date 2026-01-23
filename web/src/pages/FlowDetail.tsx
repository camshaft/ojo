import { useEffect, useMemo, useState } from "react";
import { useParams, useSearchParams } from "react-router";
import * as Plot from "@observablehq/plot";
import { Chart } from "../components/Chart";
import { useEventTypes, useQuery } from "../hooks";

type GraphType = "timeline" | "bar" | "metric";

type MetricBucket = {
  eventType: string;
  startTime: number;
  endTime: number;
  midTime: number;
  min: number;
  max: number;
  count: number;
};

enum ValueType {
  None = 0,
  Identifier = 1,
  Count = 2,
  Bytes = 3,
  Duration = 4,
  RangeCount = 5,
  RangeBytes = 6,
  RangeDuration = 7,
}

const GRAPH_OPTIONS: { id: GraphType; label: string; description: string }[] = [
  {
    id: "timeline",
    label: "Timeline",
    description: "Dots over time grouped by event type",
  },
  {
    id: "bar",
    label: "Counts",
    description: "Total events per type in this flow",
  },
  {
    id: "metric",
    label: "Metric values",
    description: "Plot payload values over time (shared Y domain)",
  },
];

const MAX_POINTS = 5000;
const TARGET_EVENTS_PER_BUCKET = 50;
const MAX_METRIC_BUCKETS = 900;

function createDurationFormat(base: number): (ms: number) => string {
  return (ms: number) => {
    ms = ms - base;
    if (ms < 1) return `${ms.toFixed(3)} ms`;
    if (ms < 1_000) return `${ms.toFixed(2)} ms`;
    const sec = ms / 1_000;
    if (sec < 60) return `${sec.toFixed(2)} s`;
    const minutes = Math.floor(sec / 60);
    const remaining = sec - minutes * 60;
    return `${minutes}m ${remaining.toFixed(1)}s`;
  };
}

export function FlowDetail() {
  const { batchId = "", flowId = "" } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const [rangeInputs, setRangeInputs] = useState<{
    start: string;
    end: string;
  } | null>(null);
  const flows = useQuery({ kind: "flows" });
  const eventTypes = useEventTypes();

  const eventsTable = useQuery({
    kind: "events",
    flow_ids: batchId !== null && flowId !== null ? [[batchId, flowId]] : [],
  });

  const flowRow = useMemo(() => {
    if (!flows) return null;
    return (
      flows.toArray().find((r) => {
        const obj = r.toJSON();
        return (
          String(obj.batch_id) === batchId && String(obj.flow_id) === flowId
        );
      }) || null
    );
  }, [flows, batchId, flowId]);

  const events = useMemo(
    () => (eventsTable ? eventsTable.toArray().map((row) => row.toJSON()) : []),
    [eventsTable],
  );

  const flow = flowRow?.toJSON();

  const availableEventTypes = useMemo(() => {
    const counts = new Map<string, number>();
    for (const event of events) {
      const key = String(event.event_type);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return Array.from(counts.entries())
      .map(([id, count]) => ({
        id,
        count,
        name: eventTypes.get(id)?.name || `Event ${id}`,
      }))
      .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
  }, [events, eventTypes]);

  const availableEventIds = useMemo(
    () => new Set(availableEventTypes.map((e) => e.id)),
    [availableEventTypes],
  );

  const searchParamString = searchParams.toString();
  const graphParam = searchParams.get("graph");
  const graphType: GraphType = (GRAPH_OPTIONS.find((o) => o.id === graphParam)
    ?.id ?? "timeline") as GraphType;

  const rangeParam = useMemo(() => {
    const from = searchParams.get("from");
    const to = searchParams.get("to");
    const parsedFrom = from !== null ? Number(from) : null;
    const parsedTo = to !== null ? Number(to) : null;
    return {
      from: Number.isFinite(parsedFrom) ? parsedFrom : null,
      to: Number.isFinite(parsedTo) ? parsedTo : null,
    };
  }, [searchParamString]);

  const rawEventsParam = useMemo(
    () => searchParams.get("events"),
    [searchParamString],
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

  useEffect(() => {
    if (!availableEventTypes.length) return;
    const params = new URLSearchParams(searchParams);
    const normalizedEvents = orderedSelectedEvents.join(",");
    const currentEventsRaw = searchParams.get("events") || "";
    const currentEventsNormalized = currentEventsRaw
      .split(",")
      .map((v) => v.trim())
      .filter((id) => availableEventIds.has(id))
      .join(",");

    let changed = false;
    if (currentEventsNormalized !== normalizedEvents) {
      params.set("events", normalizedEvents);
      changed = true;
    }

    const hasGraphParam = GRAPH_OPTIONS.some((o) => o.id === graphParam);
    if (!hasGraphParam) {
      params.set("graph", graphType);
      changed = true;
    }

    if (changed) {
      setSearchParams(params, { replace: true });
    }
  }, [
    availableEventIds,
    availableEventTypes.length,
    graphParam,
    graphType,
    orderedSelectedEvents,
    searchParams,
    setSearchParams,
  ]);

  const updateParams = (next: { events?: string[]; graph?: GraphType }) => {
    const params = new URLSearchParams(searchParams);
    if (next.events) {
      const valid = next.events.filter((id) => availableEventIds.has(id));
      if (valid.length > 0) {
        params.set("events", valid.join(","));
      }
    }
    if (next.graph) {
      params.set("graph", next.graph);
    }
    setSearchParams(params);
  };

  const updateRangeParams = (fromMs: number | null, toMs: number | null) => {
    const params = new URLSearchParams(searchParams);
    if (fromMs !== null) {
      params.set("from", String(fromMs));
    } else {
      params.delete("from");
    }
    if (toMs !== null) {
      params.set("to", String(toMs));
    } else {
      params.delete("to");
    }
    setSearchParams(params);
  };

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
    updateParams({ events: ordered, graph: graphType });
  };

  const selectAll = () => {
    const all = availableEventTypes.map((e) => e.id);
    updateParams({ events: all, graph: graphType });
  };

  const filteredEvents = useMemo(() => {
    const selectedSet = new Set(orderedSelectedEvents);
    return events.filter((event) => selectedSet.has(String(event.event_type)));
  }, [events, orderedSelectedEvents]);

  const chartData = useMemo(() => {
    const timelineData = filteredEvents.map((event: any) => {
      const id = String(event.event_type);
      const name = eventTypes.get(id)?.name || `Event ${id}`;
      return {
        timeMs: Number(event.ts_delta_ns) / 1_000_000,
        eventType: name,
      };
    });

    const metricRaw = filteredEvents
      .map((event: any) => {
        const id = String(event.event_type);
        const name = eventTypes.get(id)?.name || `Event ${id}`;
        const value = Number(event.primary_value);
        const secondaryValue = Number(event.secondary_value);
        let valueType = Number(event.value_type);
        if (valueType < 0 || valueType > 7) {
          valueType = ValueType.None;
        }
        if (!Number.isFinite(value)) return null;
        return {
          timeMs: Number(event.ts_delta_ns) / 1_000_000,
          eventType: name,
          value,
          secondaryValue,
          valueType,
        };
      })
      .filter(Boolean) as {
      timeMs: number;
      eventType: string;
      value: number;
      secondaryValue: number;
      valueType: ValueType;
    }[];

    const metricBuckets: MetricBucket[] = [];
    let metricBucketWidthMs = 0;

    if (metricRaw.length > 0) {
      const minTime = Math.min(...metricRaw.map((d) => d.timeMs));
      const maxTime = Math.max(...metricRaw.map((d) => d.timeMs));
      const duration = Math.max(maxTime - minTime, 1);
      const estimatedBuckets = Math.ceil(
        metricRaw.length / TARGET_EVENTS_PER_BUCKET,
      );
      const bucketCount =
        metricRaw.length < MAX_POINTS
          ? metricRaw.length
          : Math.min(MAX_METRIC_BUCKETS, Math.max(1, estimatedBuckets));
      metricBucketWidthMs = duration / bucketCount;

      const map = new Map<string, MetricBucket>();

      for (const d of metricRaw) {
        const idx = Math.min(
          bucketCount - 1,
          Math.floor((d.timeMs - minTime) / metricBucketWidthMs),
        );
        const startTime = minTime + idx * metricBucketWidthMs;
        const endTime = startTime + metricBucketWidthMs;
        const key = `${d.eventType}-${idx}`;
        const existing = map.get(key);

        const minValue = d.value;
        const maxValue =
          d.valueType == ValueType.RangeBytes ||
          d.valueType == ValueType.RangeCount ||
          d.valueType == ValueType.RangeDuration
            ? d.secondaryValue
            : d.value;

        if (!existing) {
          map.set(key, {
            eventType: d.eventType,
            startTime,
            endTime,
            midTime: startTime + metricBucketWidthMs / 2,
            min: minValue,
            max: maxValue,
            count: 1,
          });
        } else {
          existing.min = Math.min(existing.min, minValue);
          existing.max = Math.max(existing.max, maxValue);
          existing.count += 1;
        }
      }

      metricBuckets.push(
        ...Array.from(map.values()).sort(
          (a, b) =>
            a.midTime - b.midTime || a.eventType.localeCompare(b.eventType),
        ),
      );
    }

    const counts = availableEventTypes
      .filter((et) => orderedSelectedEvents.includes(et.id))
      .map((et) => ({
        eventType: et.name,
        count: et.count,
      }));

    const allTimes = timelineData.map((d) => d.timeMs);
    const timeDomain =
      allTimes.length > 0
        ? { min: Math.min(...allTimes), max: Math.max(...allTimes) }
        : null;

    return {
      timelineData,
      counts,
      metricBuckets,
      metricBucketWidthMs,
      total: filteredEvents.length,
      timeDomain,
    };
  }, [availableEventTypes, eventTypes, filteredEvents, orderedSelectedEvents]);

  const height = 400;
  const formatDurationMs = createDurationFormat(
    flow ? Number(flow.start_ts) / 1_000_000 : 0,
  );

  const effectiveRange = useMemo(() => {
    if (!chartData.timeDomain) return null;
    const { min, max } = chartData.timeDomain;
    let start = min;
    let end = max;
    if (rangeParam.from !== null && rangeParam.from < end) {
      start = Math.max(min, rangeParam.from);
    }
    if (rangeParam.to !== null && rangeParam.to > start) {
      end = Math.min(max, rangeParam.to);
    }
    if (end <= start) return { start: min, end: max };
    return { start, end };
  }, [chartData.timeDomain, rangeParam]);

  useEffect(() => {
    if (!effectiveRange) return;
    setRangeInputs({
      start: effectiveRange.start.toFixed(1),
      end: effectiveRange.end.toFixed(1),
    });
  }, [effectiveRange]);

  const chartOptions = useMemo(() => {
    const xDomain = effectiveRange
      ? [effectiveRange.start, effectiveRange.end]
      : chartData.timeDomain
        ? [chartData.timeDomain.min, chartData.timeDomain.max]
        : undefined;

    const metricBucketsInRange =
      graphType === "metric" && effectiveRange
        ? chartData.metricBuckets.filter(
            (b) =>
              b.endTime >= effectiveRange.start &&
              b.startTime <= effectiveRange.end,
          )
        : chartData.metricBuckets;

    if (graphType === "metric") {
      return {
        height,
        marginLeft: 70,
        marginBottom: 40,
        x: {
          label: "Time since start",
          tickFormat: formatDurationMs,
          domain: xDomain,
        },
        y: { label: "Metric value" },
        color: { legend: true },
        marks: [
          Plot.ruleX(metricBucketsInRange, {
            x: "midTime",
            y1: "min",
            y2: "max",
            stroke: "eventType",
            strokeOpacity: 0.6,
            strokeWidth: 2.5,
            title: (d: any) =>
              `${d.eventType}\n${formatDurationMs(d.startTime)} - ${formatDurationMs(d.endTime)}\nmin ${d.min}, max ${d.max} (${d.count} events)`,
          }),
          Plot.dot(
            metricBucketsInRange.filter((b) => b.min === b.max),
            {
              x: "midTime",
              y: "min",
              fill: "eventType",
              stroke: "eventType",
              r: 3,
              opacity: 0.6,
              strokeOpacity: 0.6,
              title: (d: any) =>
                `${d.eventType} @ ${formatDurationMs(d.startTime)} = ${d.min} (${d.count} events)`,
            },
          ),
        ],
      } satisfies Plot.PlotOptions;
    }

    if (graphType === "bar") {
      return {
        height,
        marginLeft: 150,
        x: { label: "Count" },
        y: { label: "Event Type" },
        marks: [
          Plot.barX(chartData.counts, {
            y: "eventType",
            x: "count",
            fill: "#4f46e5",
          }),
          Plot.text(chartData.counts, {
            y: "eventType",
            x: "count",
            text: "count",
            dx: 4,
            textAnchor: "start",
          }),
        ],
      } satisfies Plot.PlotOptions;
    }

    return {
      height,
      marginLeft: 120,
      marginBottom: 40,
      x: { label: "Time since start", tickFormat: formatDurationMs },
      y: { label: "Event Type" },
      color: { legend: true },
      marks: [
        Plot.ruleX(chartData.timelineData, {
          x: "timeMs",
          strokeOpacity: 0.05,
        }),
        Plot.dot(chartData.timelineData, {
          x: "timeMs",
          y: "eventType",
          fill: "eventType",
          stroke: "eventType",
          r: 2,
          opacity: 0.35,
          strokeOpacity: 0.35,
          title: (d: any) => `${d.eventType} @ ${formatDurationMs(d.timeMs)}`,
        }),
      ],
    } satisfies Plot.PlotOptions;
  }, [chartData, graphType]);

  const isLoading = !flows || !flowRow;

  return (
    <div className="space-y-6">
      <div className="bg-white shadow overflow-hidden sm:rounded-lg">
        <div className="px-4 py-5 sm:px-6">
          <h3 className="text-lg leading-6 font-medium text-gray-900">
            Flow Details
          </h3>
          <p className="mt-1 max-w-2xl text-sm text-gray-500">ID: {flowId}</p>
        </div>
        <div className="border-t border-gray-200 px-4 py-5 sm:p-0">
          {isLoading && (
            <div className="px-4 py-6 text-gray-500">Loading flow...</div>
          )}
          {!isLoading && flow && (
            <dl className="sm:divide-y sm:divide-gray-200">
              <div className="py-4 sm:py-5 sm:grid sm:grid-cols-3 sm:gap-4 sm:px-6">
                <dt className="text-sm font-medium text-gray-500">Batch ID</dt>
                <dd className="mt-1 text-sm text-gray-900 sm:mt-0 sm:col-span-2">
                  {new Date(Number(flow.batch_id) / 1_000_000).toLocaleString()}
                </dd>
              </div>
              <div className="py-4 sm:py-5 sm:grid sm:grid-cols-3 sm:gap-4 sm:px-6">
                <dt className="text-sm font-medium text-gray-500">
                  Event Count
                </dt>
                <dd className="mt-1 text-sm text-gray-900 sm:mt-0 sm:col-span-2">
                  {String(flow.event_count)}
                </dd>
              </div>
              <div className="py-4 sm:py-5 sm:grid sm:grid-cols-3 sm:gap-4 sm:px-6">
                <dt className="text-sm font-medium text-gray-500">Duration</dt>
                <dd className="mt-1 text-sm text-gray-900 sm:mt-0 sm:col-span-2">
                  {(
                    (Number(flow.end_ts) - Number(flow.start_ts)) /
                    1_000_000
                  ).toFixed(2)}{" "}
                  ms
                </dd>
              </div>
            </dl>
          )}
          {!isLoading && !flow && (
            <div className="px-4 py-6 text-gray-500">Flow not found</div>
          )}
        </div>
      </div>

      <div className="bg-white shadow rounded-lg p-6 space-y-6">
        <div className="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
          <div>
            <h3 className="text-lg leading-6 font-medium text-gray-900">
              Flow Visualizations
            </h3>
            <p className="text-sm text-gray-500">
              Pick which events to graph and a chart type.
            </p>
          </div>
          <div className="flex flex-wrap gap-2 items-center">
            {GRAPH_OPTIONS.map((option) => (
              <button
                key={option.id}
                type="button"
                onClick={() => updateParams({ graph: option.id })}
                className={`inline-flex items-center rounded-md border px-3 py-2 text-sm font-medium shadow-sm transition ${graphType === option.id ? "bg-indigo-600 text-white border-indigo-600" : "bg-white text-gray-700 border-gray-300 hover:bg-gray-50"}`}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>

        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <p className="text-sm font-medium text-gray-900">Event types</p>
            <button
              type="button"
              onClick={selectAll}
              className="text-sm text-indigo-600 hover:text-indigo-800"
            >
              Select all
            </button>
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
                No events available for this flow yet.
              </span>
            )}
          </div>
        </div>

        {graphType === "metric" && chartData.timeDomain && (
          <div className="space-y-3 border-t border-gray-100 pt-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <p className="text-sm font-medium text-gray-900">Time range</p>
                <p className="text-xs text-gray-500">
                  Narrow the X domain to focus on dense regions without losing
                  min/max buckets.
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-2 text-xs text-gray-500">
                <span>
                  Domain: {formatDurationMs(chartData.timeDomain.min)} –{" "}
                  {formatDurationMs(chartData.timeDomain.max)}
                </span>
              </div>
            </div>
            <div className="flex flex-wrap items-center gap-3">
              <label className="flex items-center gap-1 text-sm text-gray-700">
                Start (ms)
                <input
                  type="range"
                  step="0.1"
                  className="w-28 rounded border border-gray-300 px-2 py-1 text-sm"
                  min={chartData.timeDomain.min}
                  max={chartData.timeDomain.max}
                  value={rangeInputs?.start ?? ""}
                  onChange={(e) =>
                    setRangeInputs((prev) => ({
                      start: e.target.value,
                      end: prev?.end ?? "",
                    }))
                  }
                />
              </label>
              <label className="flex items-center gap-1 text-sm text-gray-700">
                End (ms)
                <input
                  type="range"
                  step="0.1"
                  className="w-28 rounded border border-gray-300 px-2 py-1 text-sm"
                  min={chartData.timeDomain.min}
                  max={chartData.timeDomain.max}
                  value={rangeInputs?.end ?? ""}
                  onChange={(e) =>
                    setRangeInputs((prev) => ({
                      start: prev?.start ?? "",
                      end: e.target.value,
                    }))
                  }
                />
              </label>
              <button
                type="button"
                className="inline-flex items-center rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-700"
                onClick={() => {
                  const start = Number(rangeInputs?.start ?? "");
                  const end = Number(rangeInputs?.end ?? "");
                  const startValid = Number.isFinite(start);
                  const endValid = Number.isFinite(end);
                  updateRangeParams(
                    startValid ? start : null,
                    endValid ? end : null,
                  );
                }}
              >
                Apply
              </button>
              <button
                type="button"
                className="inline-flex items-center rounded-md border border-gray-300 px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
                onClick={() => updateRangeParams(null, null)}
              >
                Reset
              </button>
            </div>
          </div>
        )}

        <div>
          {!eventsTable && (
            <div className="text-gray-500">Loading events...</div>
          )}
          {eventsTable && filteredEvents.length === 0 && (
            <div className="text-gray-500">No matching events to display.</div>
          )}
          {eventsTable && filteredEvents.length > 0 && (
            <>
              {graphType === "metric" && chartData.metricBuckets.length > 0 && (
                <div className="text-xs text-gray-500 mb-2">
                  Aggregated into ~{chartData.metricBuckets.length} buckets (
                  {chartData.metricBucketWidthMs.toFixed(1)} ms each)
                </div>
              )}
              <Chart options={chartOptions} />
            </>
          )}
        </div>
      </div>

      <div className="bg-white shadow rounded-lg p-6">
        <h3 className="text-lg leading-6 font-medium text-gray-900 mb-4">
          Flow Events
        </h3>
        {!eventsTable && <div className="text-gray-500">Loading events...</div>}
        {eventsTable && events.length === 0 && (
          <div className="text-gray-500">No events found for this flow.</div>
        )}
        {eventsTable && events.length > 0 && (
          <div className="overflow-x-auto">
            <table className="min-w-full divide-y divide-gray-200">
              <thead>
                <tr>
                  <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Time (ms)
                  </th>
                  <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Event
                  </th>
                  <th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
                    Payload
                  </th>
                </tr>
              </thead>
              <tbody className="bg-white divide-y divide-gray-200">
                {events.slice(0, 500).map((event: any, idx: number) => {
                  const eventType = eventTypes.get(String(event.event_type));
                  return (
                    <tr
                      key={`${event.ts_delta_ns}-${idx}`}
                      className="hover:bg-gray-50"
                    >
                      <td className="px-4 py-2 whitespace-nowrap text-sm text-gray-900">
                        {(Number(event.ts_delta_ns) / 1_000_000).toFixed(2)}
                      </td>
                      <td className="px-4 py-2 whitespace-nowrap text-sm text-gray-900">
                        {eventType?.name || `Event ${event.event_type}`}
                      </td>
                      <td className="px-4 py-2 whitespace-nowrap text-sm text-gray-900">
                        {String(event.primary_value)}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            {events.length > 500 && (
              <div className="text-xs text-gray-500 mt-2">
                Showing first 500 events. Narrow filters to see more.
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
