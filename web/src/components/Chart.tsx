import { useEffect, useRef } from "react";
import * as Plot from "@observablehq/plot";

interface ChartProps {
  options: Plot.PlotOptions;
}

export function Chart({ options }: ChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!containerRef.current) return;
    const plot = Plot.plot(options);
    containerRef.current.append(plot);
    return () => plot.remove();
  }, [options]);

  return <div ref={containerRef} />;
}
