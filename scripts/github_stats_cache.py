#!/usr/bin/python3

import json
import os
import hashlib
from datetime import datetime, timedelta
from typing import Dict, Optional, Any

class GitHubStatsCache:
    """
    Simple file-based cache for GitHub API responses.
    Caches data with expiration to avoid stale data.
    """
    
    def __init__(self, cache_dir: str = ".github_stats_cache", expiry_hours: int = 6):
        self.cache_dir = cache_dir
        self.expiry_hours = expiry_hours
        
        # Create cache directory if it doesn't exist
        if not os.path.exists(self.cache_dir):
            os.makedirs(self.cache_dir)
    
    def _get_cache_key(self, key: str) -> str:
        """Generate a safe filename from a cache key"""
        return hashlib.md5(key.encode()).hexdigest()
    
    def _get_cache_path(self, key: str) -> str:
        """Get the full path to a cache file"""
        cache_key = self._get_cache_key(key)
        return os.path.join(self.cache_dir, f"{cache_key}.json")
    
    def get(self, key: str) -> Optional[Any]:
        """Get a value from the cache if it exists and isn't expired"""
        cache_path = self._get_cache_path(key)
        
        if not os.path.exists(cache_path):
            return None
        
        try:
            with open(cache_path, 'r') as f:
                cache_data = json.load(f)
            
            # Check if cache is expired
            cached_time = datetime.fromisoformat(cache_data['timestamp'])
            if datetime.now() - cached_time > timedelta(hours=self.expiry_hours):
                # Cache is expired, remove it
                os.remove(cache_path)
                return None
            
            return cache_data['data']
        except (json.JSONDecodeError, KeyError, ValueError):
            # Invalid cache file, remove it
            if os.path.exists(cache_path):
                os.remove(cache_path)
            return None
    
    def set(self, key: str, value: Any) -> None:
        """Store a value in the cache"""
        cache_path = self._get_cache_path(key)
        
        cache_data = {
            'timestamp': datetime.now().isoformat(),
            'data': value
        }
        
        with open(cache_path, 'w') as f:
            json.dump(cache_data, f)
    
    def clear(self) -> None:
        """Clear all cached data"""
        for filename in os.listdir(self.cache_dir):
            if filename.endswith('.json'):
                os.remove(os.path.join(self.cache_dir, filename))