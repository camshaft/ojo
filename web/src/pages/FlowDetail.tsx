import { useMemo } from "react";
import { useParams } from "react-router";
import * as Plot from "@observablehq/plot";
import { Chart } from "../components/Chart";
import { useFlowData, GRAPH_OPTIONS } from "../hooks/useFlowData";

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
  
  const {
    flow,
    events,
    eventsTable,
    filteredEvents,
    chartData,
    effectiveRange,
    eventTypes,
    availableEventTypes,
    orderedSelectedEvents,
    graphType,
    rangeInputs,
    updateParams,
    handleRangeInputChange,
    toggleEvent,
    selectAll,
    resetRange,
    isLoading,
  } = useFlowData(batchId, flowId);

  const height = 400;
  const formatDurationMs = createDurationFormat(
    flow ? Number(flow.start_ts) / 1_000_000 : 0,
  );

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
            strokeWidth: 10.5,
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
  }, [chartData, graphType, effectiveRange, flow]);

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
            <div className="flex flex-col gap-4">
              <label className="flex flex-col gap-2 text-sm text-gray-700">
                <div className="flex items-center justify-between">
                  <span>Start (ms)</span>
                  <span className="text-xs font-mono text-gray-500">
                    {formatDurationMs(Number(rangeInputs?.start ?? 0))}
                  </span>
                </div>
                <input
                  type="range"
                  step="0.1"
                  className="w-full"
                  min={chartData.timeDomain.min}
                  max={chartData.timeDomain.max}
                  value={rangeInputs?.start ?? ""}
                  onChange={(e) =>
                    handleRangeInputChange("start", e.target.value)
                  }
                />
              </label>
              <label className="flex flex-col gap-2 text-sm text-gray-700">
                <div className="flex items-center justify-between">
                  <span>End (ms)</span>
                  <span className="text-xs font-mono text-gray-500">
                    {formatDurationMs(Number(rangeInputs?.end ?? 0))}
                  </span>
                </div>
                <input
                  type="range"
                  step="0.1"
                  className="w-full"
                  min={chartData.timeDomain.min}
                  max={chartData.timeDomain.max}
                  value={rangeInputs?.end ?? ""}
                  onChange={(e) =>
                    handleRangeInputChange("end", e.target.value)
                  }
                />
              </label>
              <button
                type="button"
                className="inline-flex items-center justify-center rounded-md border border-gray-300 px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
                onClick={resetRange}
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
