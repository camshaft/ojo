import { useQuery, useEventTypes } from "../hooks";
import { Chart } from "../components/Chart";
import { Link } from "react-router";
import * as Plot from "@observablehq/plot";

export function Dashboard() {
  const stats = useQuery({ kind: "stats" });
  const flows = useQuery({ kind: "flows" });
  const eventTypes = useEventTypes();

  const statsData = stats ? stats.toArray().map((r) => r.toJSON()) : [];
  const flowsData = flows ? flows.toArray().map((r) => r.toJSON()) : [];

  return (
    <div className="space-y-6">
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
