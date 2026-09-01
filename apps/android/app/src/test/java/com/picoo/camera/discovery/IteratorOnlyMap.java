package com.picoo.camera.discovery;

import java.util.AbstractMap;
import java.util.AbstractSet;
import java.util.Iterator;
import java.util.Map;
import java.util.Set;

/** Map whose entry set matches Android's iterator-only NSD attribute behavior. */
final class IteratorOnlyMap<K, V> extends AbstractMap<K, V> {
    private final Map<K, V> delegate;

    IteratorOnlyMap(Map<K, V> delegate) {
        this.delegate = delegate;
    }

    @Override
    public Set<Entry<K, V>> entrySet() {
        return new AbstractSet<>() {
            @Override
            public Iterator<Entry<K, V>> iterator() {
                return delegate.entrySet().iterator();
            }

            @Override
            public int size() {
                return delegate.size();
            }

            @Override
            public Object[] toArray() {
                throw new UnsupportedOperationException("platform entry set does not expose toArray");
            }

            @Override
            public <T> T[] toArray(T[] array) {
                throw new UnsupportedOperationException("platform entry set does not expose toArray");
            }
        };
    }
}
