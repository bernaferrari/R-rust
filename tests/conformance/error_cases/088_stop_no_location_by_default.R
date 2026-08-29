# With show.error.locations unset (the default) a top-level stop renders
# plain "Error: <message>" — no "(from #n)" location marker anywhere.
print(1)
print(2)
stop('boom')
