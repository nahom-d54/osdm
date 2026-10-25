/** Complete every selected operation before reporting partial failure. */
export async function runBulk(
  ids: string[],
  action: (id: string) => Promise<unknown>,
): Promise<void> {
  const results = await Promise.allSettled(
    ids.map((id) => Promise.resolve().then(() => action(id))),
  );
  const failed = results.filter(
    (result) => result.status === "rejected",
  ).length;
  if (failed)
    throw new Error(
      `${failed} of ${ids.length} operations failed. Refresh the list and retry the remaining entries.`,
    );
}
