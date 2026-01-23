import { useParams } from "react-router";
import { useEventTypes, useQuery } from "../hooks";

export function FlowDetail() {
  const { batchId = "", flowId = "" } = useParams();
  const flows = useQuery({ kind: "flows" });
  const eventTypes = useEventTypes();

  const eventsTable = useQuery({
    kind: "events",
    flow_ids: batchId !== null && flowId !== null ? [[batchId, flowId]] : [],
  });

  if (!flows) return <div>Loading...</div>;

  const flowRow = flows.toArray().find((r) => {
    const obj = r.toJSON();
    return String(obj.batch_id) === batchId && String(obj.flow_id) === flowId;
  });

  if (!flowRow) {
    return <div>Flow not found</div>;
  }

  const events = eventsTable
    ? eventsTable.toArray().map((row) => row.toJSON())
    : [];

  const flow = flowRow.toJSON();
  console.log(events);

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
          <dl className="sm:divide-y sm:divide-gray-200">
            <div className="py-4 sm:py-5 sm:grid sm:grid-cols-3 sm:gap-4 sm:px-6">
              <dt className="text-sm font-medium text-gray-500">Batch ID</dt>
              <dd className="mt-1 text-sm text-gray-900 sm:mt-0 sm:col-span-2">
                {new Date(Number(flow.batch_id) / 1_000_000).toLocaleString()}
              </dd>
            </div>
            <div className="py-4 sm:py-5 sm:grid sm:grid-cols-3 sm:gap-4 sm:px-6">
              <dt className="text-sm font-medium text-gray-500">Event Count</dt>
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
                        {String(event.payload)}
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
