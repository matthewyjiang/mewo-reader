<script lang="ts">
  import FilterMenu from './FilterMenu.svelte';
  import type { CatalogFilter, CatalogFilters } from '../lib/models';

  let { filters, filter, searching, onchange }: {
    filters: CatalogFilters;
    filter: CatalogFilter;
    /** Typed text searches. Sort applies to browse only. */
    searching: boolean;
    onchange: (filter: CatalogFilter) => void;
  } = $props();

  let genreItems = $derived([{ id: null, name: 'All genres' }, ...filters.genres]);
  let genreLabel = $derived(filters.genres.find((genre) => genre.id === filter.genre)?.name ?? 'All genres');
  let firstSort = $derived(filters.sorts[0]?.id ?? null);
  let sortLabel = $derived(
    filters.sorts.find((sort) => sort.id === (filter.sort ?? firstSort))?.name ?? '',
  );
</script>

<div class="filters">
  <FilterMenu
    label={genreLabel}
    items={genreItems}
    selected={filter.genre}
    testid="catalog-genre"
    onselect={(genre) => onchange({ ...filter, genre })}
  />
  {#if !searching && filters.sorts.length}
    <FilterMenu
      label={sortLabel}
      items={filters.sorts}
      selected={filter.sort ?? firstSort}
      testid="catalog-sort"
      onselect={(sort) => onchange({ ...filter, sort: sort === firstSort ? null : sort })}
    />
  {/if}
</div>

<style>
  .filters {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px;
    min-width: 0;
  }
</style>
