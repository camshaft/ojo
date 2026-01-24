import { useMemo, useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router";
import { useEventTypes, useQuery, ValueType } from "../hooks";

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

type AvailableEventType = {
  id: string;
  count: number;
  name: string;
};

type ChartData = {
  timelineData: Array<{ timeMs: number; eventType: string }>;
  counts: Array<{ eventType: string; count: number }>;
  metricBuckets: MetricBucket[];
  metricBucketWidthMs: number;
  total: number;
  timeDomain: { min: number; max: number } | null;
};

type EffectiveRange = {
  start: number;
  end: number;
} | null;

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

export function useFlowData(batchId: string, flowId: string) {
  const [searchParams, setSearchParams] = useSearchParams();
  const [rangeInputs, setRangeInputs] = useState<{
    start: string;
    end: string;
  } | null>(null);
  const rangeUpdateTimeout = useRef<number | null>(null);
  
  const flows = useQuery({ kind: "flows" });
  const eventTypes = useEventTypes();

  const eventsTable = useQuery({
    kind: "events",
    flow_ids: batchId !== "" && flowId !== "" ? [[batchId, flowId]] : [],
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

  const scheduleRangeParams = (fromMs: number | null, toMs: number | null) => {
    if (rangeUpdateTimeout.current !== null) {
      window.clearTimeout(rangeUpdateTimeout.current);
    }
    rangeUpdateTimeout.current = window.setTimeout(() => {
      updateRangeParams(fromMs, toMs);
      rangeUpdateTimeout.current = null;
    }, 150);
  };

  const handleRangeInputChange = (field: "start" | "end", newValue: string) => {
    setRangeInputs((prev) => {
      const start =
        field === "start" ? Number(newValue) : Number(prev?.start ?? "");
      const end = field === "end" ? Number(newValue) : Number(prev?.end ?? "");
      const startValid = Number.isFinite(start);
      const endValid = Number.isFinite(end);
      scheduleRangeParams(startValid ? start : null, endValid ? end : null);
      return {
        start: field === "start" ? newValue : (prev?.start ?? ""),
        end: field === "end" ? newValue : (prev?.end ?? ""),
      };
    });
  };

  useEffect(
    () => () => {
      if (rangeUpdateTimeout.current !== null) {
        window.clearTimeout(rangeUpdateTimeout.current);
      }
    },
    [],
  );

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

  const chartData: ChartData = useMemo(() => {
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
        const ty = eventTypes.get(id);
        const name = ty?.name || `Event ${id}`;
        const value = Number(event.primary_value);
        const secondaryValue = Number(event.secondary_value);
        let valueType = ty?.value_type || ValueType.None;
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

  const effectiveRange: EffectiveRange = useMemo(() => {
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

  const resetRange = () => {
    if (rangeUpdateTimeout.current !== null) {
      window.clearTimeout(rangeUpdateTimeout.current);
      rangeUpdateTimeout.current = null;
    }
    updateRangeParams(null, null);
  };

  const isLoading = !flows || !flowRow;

  return {
    // Data
    flow,
    flowRow,
    flows,
    events,
    eventsTable,
    filteredEvents,
    chartData,
    effectiveRange,
    
    // Event types
    eventTypes,
    availableEventTypes,
    availableEventIds,
    orderedSelectedEvents,
    
    // URL params
    graphType,
    rangeParam,
    rangeInputs,
    
    // Actions
    updateParams,
    updateRangeParams,
    handleRangeInputChange,
    toggleEvent,
    selectAll,
    resetRange,
    
    // Loading state
    isLoading,
  };
}

export { GRAPH_OPTIONS };
export type { GraphType, MetricBucket, AvailableEventType, ChartData, EffectiveRange };
