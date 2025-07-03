#!/usr/bin/python3

import asyncio
import os
import time

import aiohttp

from github_stats import Stats


async def test_performance():
    """Test the performance improvements"""
    access_token = os.getenv("ACCESS_TOKEN")
    if not access_token:
        print("ACCESS_TOKEN environment variable is required")
        return

    user = os.getenv("GITHUB_ACTOR", "nrminor")  # Default to nrminor if not set

    print(f"Testing performance for user: {user}")
    print("=" * 60)

    async with aiohttp.ClientSession() as session:
        # Test without cache (first run)
        print("\nFirst run (no cache):")
        start_time = time.time()

        s = Stats(user, access_token, session, use_cache=True)

        # Trigger all API calls
        print(f"Name: {await s.name}")
        print(f"Repos count: {len(await s.repos)}")
        print(f"Total stars: {await s.stargazers}")
        print(f"Total forks: {await s.forks}")
        print(f"Total contributions: {await s.total_contributions}")

        # These are the expensive operations
        lines = await s.lines_changed
        print(f"Lines changed: {lines[0] + lines[1]:,}")

        views = await s.views
        print(f"Views: {views}")

        first_run_time = time.time() - start_time
        print(f"\nFirst run completed in: {first_run_time:.2f} seconds")

        # Test with cache (second run)
        print("\n" + "=" * 60)
        print("\nSecond run (with cache):")
        start_time = time.time()

        s2 = Stats(user, access_token, session, use_cache=True)

        # Trigger same API calls
        await s2.name
        await s2.repos
        await s2.stargazers
        await s2.forks
        await s2.total_contributions
        await s2.lines_changed
        await s2.views

        second_run_time = time.time() - start_time
        print(f"\nSecond run completed in: {second_run_time:.2f} seconds")

        # Show improvement
        improvement = ((first_run_time - second_run_time) / first_run_time) * 100
        print(f"\nPerformance improvement: {improvement:.1f}%")
        print(f"Speedup: {first_run_time / second_run_time:.1f}x faster")


if __name__ == "__main__":
    asyncio.run(test_performance())
