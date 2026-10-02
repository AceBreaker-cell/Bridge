export const useTauri = () => {
  // Check if we're in a Tauri environment
  const isTauri = (window as any).__TAURI__ !== undefined;

  const invoke = async <T = any>(command: string, args?: any): Promise<T> => {
    // Real Tauri invocation
    return (window as any).__TAURI__.invoke(command, args);
  };

  return { invoke, isTauri };
};