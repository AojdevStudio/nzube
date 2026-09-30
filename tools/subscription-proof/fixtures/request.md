Repo: example-org/ledger-lite
Request type: Bug fix
Goal: CSV export drops the last invoice row.
Context: A customer says the exported file is missing the final invoice when the source list has an odd number of rows. I have not reproduced it. I think it might be the pagination helper but I'm not sure.
Done means: Tested, pushed PR
Constraints: Do not change the CSV column order. Keep the export streaming; don't load everything into memory.
