package co.juandevgo.paleoapi.infrastructure.persistence.repository;

import co.juandevgo.paleoapi.infrastructure.entity.SpecieEntity;
import org.springframework.data.jpa.repository.JpaRepository;
import org.springframework.data.jpa.repository.Query;
import org.springframework.data.repository.query.Param;
import org.springframework.stereotype.Repository;

import java.util.List;

@Repository
public interface SpeciesRepositoryJPA extends JpaRepository<SpecieEntity, Long> {

    List<SpecieEntity> findAllByName(String name);

    List<SpecieEntity> findAllByGroup(String group);

    List<SpecieEntity> findAllByTimePeriod(String timePeriod);

    @Query(value = "SELECT * FROM species s WHERE EXISTS (SELECT 1 FROM unnest(s.locations) AS loc WHERE loc ILIKE %:location%)", nativeQuery = true)
    List<SpecieEntity> findAllByLocation(@Param("location") String location);

    @Query(value = "SELECT * FROM species s WHERE EXISTS (SELECT 1 FROM unnest(s.classification) AS cls WHERE cls ILIKE %:class%)", nativeQuery = true)
    List<SpecieEntity> findAllByClassification(@Param("class") String classification);
}