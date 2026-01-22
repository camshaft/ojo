import { useParams } from "react-router";
import { useQuery } from "../hooks";

export function FlowDetail() {
  const { flowId } = useParams();
  const flows = useQuery({ kind: "flows" });

  // In a real app we might want to query specifically for this flow's events
  // For now we will find the flow metadata from the list
  const flow = flows
    ? flows
        .toArray()
        .map((r) => r.toJSON())
        .find((f: any) => String(f.flow_id) === flowId)
    : null;

  if (!flow) {
    if (!flows) return <div>Loading...</div>;
    return <div>Flow not found</div>;
  }

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

      {/* Placeholder for timeline - we need more detailed event data query for this */}
      <div className="bg-white shadow rounded-lg p-6">
        <h3 className="text-lg leading-6 font-medium text-gray-900 mb-4">
          Flow Timeline
        </h3>
        <div className="text-gray-500 italic">
          To visualize the flow timeline, we would need to fetch the individual
          events for this flow. Currently the server only exposes aggregated
          stats.
        </div>
      </div>
    </div>
  );
}
